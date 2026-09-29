# Gmail (work inbox)

Kind `gmail`, tag `work`. Every unread thread in your Inbox comes into Incoming as one signal, including on the first sync, and leaves Incoming when you read or archive it in Gmail. Confirming or filing an email in Alethiometer marks its thread read in Gmail.

## 1. Google Cloud project and OAuth client

Gmail reuses the calendar's OAuth client (see [gcal.md](gcal.md) §1), so there's nothing new to create. In the same GCP project:

1. **Enable the API:** APIs & Services → Library → "Gmail API" → Enable.
2. **Data access / scopes:** add `https://www.googleapis.com/auth/gmail.modify` on the consent screen (Google Auth Platform → Data access) if it isn't listed yet. With an Internal app there's no verification step for it.

`gmail.modify` lets Alethiometer read mail and change labels (it only ever removes `UNREAD`). It can't delete mail permanently, and Alethiometer never sends mail or deletes anything.

> Workspace admins control Gmail access separately from Calendar (Admin console → Security → API controls). If consent fails with "admin_policy_enforced", ask them to allow the OAuth client for Gmail.

## 2. Secrets

| Secret name | Value |
|---|---|
| `gcal.client_id` | Shared with the calendar. Already set. |
| `gcal.client_secret` | Shared with the calendar. Already set. |
| `gmail.refresh_token` | Written by Alethiometer after consent. Don't set it by hand. |

The calendar keeps its own `gcal.refresh_token`; connecting Gmail doesn't touch it. `config.secret_prefix` (default `gmail`) renames the refresh token; `config.client_prefix` (default `gcal`) points at a different client.

## 3. Add the source to `settings.json`

```json
{
  "sources": [
    {
      "kind": "gmail",
      "tag": "work",
      "name": "Work mail",
      "poll_secs": 120,
      "config": {
        "account": "you@yourcompany.com"
      }
    }
  ]
}
```

All `config` fields are optional:

| Field | Default | Meaning |
|---|---|---|
| `account` | none | Passed to Google as `login_hint`, so consent picks the work account. Also used in links (`mail.google.com/mail/u/<account>/…`), so they open in the right account when you're signed into several. |
| `read_only` | `false` | Never mark threads read in Gmail. |
| `scopes` | `["https://www.googleapis.com/auth/gmail.modify"]` | Don't change this unless you know why. |
| `secret_prefix` | `gmail` | Prefix of the refresh token secret. |
| `client_prefix` | `gcal` | Prefix of the OAuth client secrets. |

## 4. Connect

Restart Alethiometer after editing `settings.json`. The first sync fails with "isn't connected to Google yet", and the sync status in the tab bar shows **Connect** next to the Gmail source. Press it: your browser opens Google's consent screen for `gmail.modify`. Approve with your work account and Alethiometer saves `gmail.refresh_token`, then syncs. It's the same flow as the calendar (PKCE, loopback redirect on `127.0.0.1`; see [gcal.md](gcal.md) §4).

From the dev server: `curl -s -X POST http://127.0.0.1:1421/call/gcal.auth_start -d '{"kind": "gmail"}'`, then `gcal.auth_status` with the same body. `{"source_id": N}` works too.

## What is read

- **First sync** (and whenever Google no longer has the stored history id, a `404`): `users.getProfile` for the mailbox's current `historyId`, then `users.threads.list` with `q=in:inbox is:unread`, every page (500 per page). More than 50,000 unread threads is an error rather than a partial list.
- **After that**, each poll: `users.history.list` from the stored `historyId`, every page. A thread goes on the fetch list when it's already an item, or when the change could make it unread in the Inbox (a new message with `INBOX` and `UNREAD`, or one of those labels added). Sent mail and label changes on other threads are skipped. If there are more than 100 pages of changes, the poll re-lists instead.
- Threads on the fetch list are read with `users.threads.get` (`format=full`), up to 200 per poll. The rest wait in the cursor for the next poll, and so does everything after a rate-limit answer (`429`). A huge inbox or a mass change in Gmail takes a few polls to come in, but nothing is dropped.
- The cursor (`sources.cursor`) is JSON: `{history_id, queue, tracked}`. `queue` is the fetch list; `tracked` maps each unread Inbox thread to the time of its newest unread message. No mail content is kept there.

A message counts when it has both the `UNREAD` and `INBOX` labels. A thread with at least one such message is one item of kind `email`:

| Item field | From |
|---|---|
| title | `Subject` of the thread's first message (`(no subject)` if empty) |
| author | `From` of the newest unread message: address as handle, display name (or a name from the address) as the person |
| messages | the unread messages, oldest first: time, sender name, body. Body is the `text/plain` part if there is one, else the `text/html` part with tags, styles and scripts stripped. Quoted replies ("On … wrote:", `>` lines, Outlook's "Original Message") are cut, whitespace is tidied and each body is clipped to 2,000 characters. Attachments are skipped. |
| summary | the newest unread message, first 140 characters |
| url | `https://mail.google.com/mail/u/<account or 0>/#all/<thread id>` |
| times | first and newest unread message (`internalDate`) |

- **New signal:** a new thread, or a newer unread message in a known thread (bump). Reading part of a thread in Gmail updates the item's messages without resurfacing it.
- **Read, archived, trashed or deleted in Gmail:** nothing changes here. The item stays in Incoming until you triage it; Alethiometer stops following the thread until it gets new unread mail. It is never deleted, so its topic links and history stay.

## What is written (outbox ops)

### `gmail.mark_read`

Payload: `{"thread_id": "<thread id = item external_id>", "message_ids": ["<Gmail message ids you saw>"]}`. Queued when you **Confirm** (or press **Seen** on an update) or **Move** an email to a topic in Incoming. **Dismiss** never writes to Gmail. For a manually split email, it's queued when the last part leaves Incoming.

Alethiometer first reads the thread (`format=minimal`). If unread Inbox mail arrived that isn't in `message_ids`, it leaves the thread unread and confirms the op without writing (receipt `{thread_id, left_unread}`); the next poll brings the new mail in as a bump. Otherwise it calls `users.threads.modify` with `{"removeLabelIds": ["UNREAD"]}`. Receipt: `{thread_id, history_id}`.

## Not verified without real credentials

All tests use recorded-shape fixtures (`src-tauri/tests/fixtures/gmail/`). Against the real account, check:

- consent for `gmail.modify` on the shared client (and that the calendar keeps syncing afterwards);
- that `#all/<thread id>` links open the thread in Gmail web;
- header decoding of non-ASCII subjects and senders, and bodies in charsets other than UTF-8.
