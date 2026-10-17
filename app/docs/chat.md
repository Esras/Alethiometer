# Chat sources

Slack and Microsoft Teams are *chat sources* (`settings::sources::CHAT_KINDS`). The rules the user set for Slack (HANDOFF.md, "User decisions") hold for every chat source: Incoming is for filing context to topics, not for "have I read this"; every message is captured, yours too; only triage here closes a signal; a thread is one topic and attaches to its parent message; DMs and channels mix topics, so the AI files top-level messages one by one.

This page is the contract a chat connector keeps so the shared code works for it. Each source's own page (`sources/slack.md`, `sources/teams.md`) says how it meets it.

## What is shared

| Where | What it does for any chat kind |
|---|---|
| `sync/connectors/chat.rs` | The run model: `Open` runs and `closed` streams in the cursor (a connector's cursor type implements `chat::Runs`), and `close_triaged`, which the connector calls from `Connector::before_poll`. |
| `sync::Connector` | Hooks with defaults: `before_poll` (the cursor to poll from), `live` (whether a live feed beside polling is up, for the sync chip; `sync::start_source` starts the feed), `deep_link` (a web link as a link into the desktop app; `api.rs` `open_url` asks every chat connector). |
| `queries/incoming.rs` | Mark read on Confirm (`<kind>.mark_read`), the split review's conversation kind (`conv_kind`), reply state on rows. |
| `queries/respond.rs` | `respond.reply` queues `<kind>.reply`. |
| `queries/people.rs` | Author ids are accounts of kind `<kind>`; your own id is `me` in each chat source's cursor. |
| UI | `isChat(source)` (`lib/sources.ts`, from `app.info`'s `chat_kinds`): `r` and the Reply button (`ChatReply.svelte`), `s` opens the split review on any signal with 2+ messages. |

## The contract

**Items.** One item per run. `raw_json` holds:

- `channel`: the conversation's id. Thread replies find the item holding their parent through it (`items.holder`).
- `msgs`: each stored message's key, context messages first, in the order of the item's messages. Splits move messages by these keys.
- `thread_ts`: on a thread item, its root message's key. The item is a thread for the UI and the split review.
- `mark_ts`: the key "mark read" sends; only on items the source can mark read.
- `conv` (optional): `dm`, `group` or `channel`. Without it the conversation kind is read off Slack's titles (`DM · …`, `Group DM · …`).
- `name` (optional): the conversation's name, which ignore suggestions show (`queries/ignore.rs`); for Slack the channel name without `#`, what `muted` takes.

The field names come from Slack, the first chat source; they stay so existing items need no migration.

**Message keys** (`item_messages.ext_key`) are unique within a conversation and **sort as numbers in time order**: mark read takes the newest key you saw, comparing them as numbers. Slack's are its `ts` ("1789960000.000100"); Teams' are its message ids, which are creation times in milliseconds ("1791978000000").

**Thread replies** carry `reply_to` (the parent's key) and are stored under the parent's item when one holds it (`ItemUpsert.attach_to` = the parent's key).

**Cursor.** The connector's cursor has `open` (runs not triaged yet: `{id, first, last}` by stream key, "<channel>" or "<channel>/<root key>") and `closed` (per stream, the last key of the newest triaged run), plus `me`, your own user id on that source.

**Outbox ops**, named `<kind>.<op>`:

| Op | Payload | Meaning |
|---|---|---|
| `<kind>.mark_read` | `{channel, ts}` | Mark the conversation read up to `ts`, never backwards. Queued on Confirm once every part of a split family has left Incoming. |
| `<kind>.reply` | `{channel, thread_ts?, text}` | Post as you: in the thread of `thread_ts`, else in the conversation. Not safe to resend. |
| `<kind>.react` | `{channel, ts, name}` | Add a reaction (no UI yet). |

`mark_read` and `react` belong in `outbox::SAFE_TO_RESEND`; `reply` never does.

## Not shared

Each connector keeps its own API client, its poll engine (what a stream is, when to read it), how it renders message text, its settings and secrets, and its People-note field (`slack:`, `teams:`).
