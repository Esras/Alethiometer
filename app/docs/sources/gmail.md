# Gmail (work inbox)

Kind `gmail`, tag `work`. Every thread in your Inbox, read or not, is one signal in Incoming, including on the first sync. A new message in a thread brings it back, even when it's already filed under a topic. A thread that leaves the Inbox in Gmail (archived, trashed, deleted) leaves Incoming. Triage writes back: filing under a topic labels the thread with its category's label and marks it read, **Dismiss** archives it, **Trash** (`#`) trashes it, and undo (`u`) puts it back in the Inbox. A thread is never split across topics.

## 1. Google Cloud project and OAuth client

Gmail reuses the calendar's OAuth client (see [gcal.md](gcal.md) §1), so there's nothing new to create. In the same GCP project:

1. **Enable the API:** APIs & Services → Library → "Gmail API" → Enable.
2. **Data access / scopes:** add `https://www.googleapis.com/auth/gmail.modify` on the consent screen (Google Auth Platform → Data access) if it isn't listed yet. With an Internal app there's no verification step for it.

`gmail.modify` lets Alethiometer read mail, change labels and move threads to and from the Trash. It can't delete mail permanently, and Alethiometer never sends mail.

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
| `category_labels` | none | Category key → Gmail label name, e.g. `{"customers": "Customers", "vendors": "Vendors"}`. Filing a thread under a topic adds the label of the topic's category. The label must exist in Gmail. Categories without an entry get no label. |
| `read_only` | `false` | Never write to Gmail (no labels, read state, archive or trash). |
| `scopes` | `["https://www.googleapis.com/auth/gmail.modify"]` | Don't change this unless you know why. |
| `secret_prefix` | `gmail` | Prefix of the refresh token secret. |
| `client_prefix` | `gcal` | Prefix of the OAuth client secrets. |

## 4. Connect

Restart Alethiometer after editing `settings.json`. The first sync fails with "isn't connected to Google yet", and the sync status in the tab bar shows **Connect** next to the Gmail source. Press it: your browser opens Google's consent screen for `gmail.modify`. Approve with your work account and Alethiometer saves `gmail.refresh_token`, then syncs. It's the same flow as the calendar (PKCE, loopback redirect on `127.0.0.1`; see [gcal.md](gcal.md) §4).

From the dev server: `curl -s -X POST http://127.0.0.1:1421/call/gcal.auth_start -d '{"kind": "gmail"}'`, then `gcal.auth_status` with the same body. `{"source_id": N}` works too.

## What is read

- **First sync** (and whenever Google no longer has the stored history id, a `404`): `users.getProfile` for the mailbox's current `historyId`, then `users.threads.list` with `q=in:inbox`, every page (500 per page). More than 50,000 Inbox threads is an error rather than a partial list. Tracked threads missing from the list left the Inbox meanwhile.
- **After that**, each poll: `users.history.list` from the stored `historyId`, every page. A thread goes on the fetch list when a message arrives in it (for a tracked thread, any message, including one you sent), or when `INBOX`, `TRASH` or `SPAM` is added to or removed from it. Read-state and other label changes are skipped: they don't change what's in Incoming. If there are more than 100 pages of changes, the poll re-lists instead.
- Threads on the fetch list are read with `users.threads.get` (`format=full`), up to 200 per poll. The rest wait in the cursor for the next poll, and so does everything after a rate-limit answer (`429`). A huge inbox or a mass change in Gmail takes a few polls to come in, but nothing is dropped.
- The cursor (`sources.cursor`) is JSON: `{history_id, queue, tracked}`. `queue` is the fetch list; `tracked` maps each Inbox thread to the time of its newest message. No mail content is kept there.

A thread is in the Inbox when one of its messages has the `INBOX` label. It's one item of kind `email`, holding every message that isn't in the Trash, Spam or a draft:

| Item field | From |
|---|---|
| title | `Subject` of the thread's first message (`(no subject)` if empty) |
| author | `From` of the newest message: address as handle, display name (or a name from the address) as the person. Mail a service sends for many people from one address is traced to the person where the headers say who: Google Groups `X-Original-From` / `X-Original-Sender`, Drive shares `Reply-To`, GitHub `X-GitHub-Sender` (handle `github:<login>`). Where they don't (Jira, Docs comments; relay addresses containing `noreply`, `no-reply`, `notification`, or `jira@`/`confluence@`), the handle is the address plus the display name. |
| messages | every message, oldest first: time, sender name, body. Body is the `text/plain` part if there is one, else the `text/html` part with tags, styles and scripts stripped. Quoted replies ("On … wrote:", `>` lines, Outlook's "Original Message") are cut, whitespace is tidied and each body is clipped to 2,000 characters. Attachments are skipped. |
| summary | the newest message, first 140 characters |
| url | `https://mail.google.com/mail/u/<account or 0>/#all/<thread id>` |
| times | first and newest message (`internalDate`) |

- **New signal:** a new Inbox thread, or a newer message in a known one (bump), yours included. A filed thread comes back under its topic as an update.
- **Left the Inbox in Gmail** (archived, trashed, deleted): the item leaves Incoming (marked seen) and isn't followed any more. It's never deleted, so its topic links and history stay. If the thread comes back to the Inbox, so does the item.
- **Read or unread in Gmail** changes nothing here.

## What is written (outbox ops)

Payloads carry `thread_id` (the item's external id). Receipts: `{thread_id, history_id}` unless noted.

- **`gmail.file`** `{thread_id, message_ids, label?}`: queued when you **Confirm** or **Move** an email to a topic. `label` is the topic category's entry in `category_labels`. Alethiometer first reads the thread (`format=minimal`). If unread Inbox mail arrived that isn't in `message_ids`, the thread stays unread (receipt adds `left_unread`) and the next poll brings the new mail in as a bump. Then one `users.threads.modify`: remove `UNREAD` and add the label (looked up by name with `users.labels.list`; a missing label fails the op, "create it in Gmail, then retry"). Rows queued as `gmail.mark_read` by older builds run the same way.
- **`gmail.archive`**: **Dismiss**. Removes `INBOX`.
- **`gmail.trash`**: **Trash** (`#`, Gmail items only). `users.threads.trash`; Gmail empties the Trash after 30 days.
- **`gmail.restore`**: undo (`u`) after Dismiss or Trash. `users.threads.untrash`, then adds `INBOX` back.

Later, with to-dos: filing will also archive the thread, and a thread that needs a follow-up stays in the Inbox until its to-do is done.

## Not verified without real credentials

All tests use recorded-shape fixtures (`src-tauri/tests/fixtures/gmail/`). Against the real account, check:

- consent for `gmail.modify` on the shared client (and that the calendar keeps syncing afterwards);
- that `#all/<thread id>` links open the thread in Gmail web;
- header decoding of non-ASCII subjects and senders, and bodies in charsets other than UTF-8.
