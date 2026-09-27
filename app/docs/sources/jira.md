# Jira (Jira Cloud)

Kind `jira`, tag `work`. Connector: `src-tauri/src/sync/connectors/jira.rs`.

## settings.json

```json
{
  "sources": [
    {
      "kind": "jira",
      "tag": "work",
      "name": "Work Jira",
      "poll_secs": 300,
      "config": {
        "base_url": "https://acme.atlassian.net",
        "email": "you@acme.com",
        "create_project": "PLAT",
        "create_issue_type": "Task",
        "jql": "assignee = currentUser() OR reporter = currentUser() OR watcher = currentUser()",
        "lookback_days": 14,
        "secret": "jira_api_token"
      }
    }
  ]
}
```

| Key | Required | Meaning |
|---|---|---|
| `base_url` | yes | Your site, `https://<site>.atlassian.net` |
| `email` | yes | The Atlassian account email that owns the API token |
| `create_project` | for Notes → Jira | Project key new issues from Notes go into |
| `create_issue_type` | no (`Task`) | Issue type name for those issues |
| `jql` | no | Which issues to follow. Default shown above. Helm adds `AND updated >= "…" ORDER BY updated ASC` |
| `lookback_days` | no (14) | How far back the first sync reaches |
| `secret` | no (`jira_api_token`) | Name of the secret holding the API token |

## Secret

Store the API token under **`jira_api_token`** (or the name in `config.secret`):
Keychain on the Mac, `secrets.json` (mode 0600) in the data dir on Linux.

Create it at <https://id.atlassian.com/manage-profile/security/api-tokens>. Use a classic
API token (not a scoped one): the connector authenticates with HTTP basic auth
(`email:token`) against your site URL. The token acts as you, so it can do what your
account can: *Browse projects* to read, *Transition issues*, *Add comments*,
*Assign issues* and *Create issues* in the relevant projects to write.

## What is read

Every poll:

1. `GET /rest/api/3/myself`, for your `accountId` (to tell "assigned to you" and ignore your own changes).
2. `GET /rest/api/3/search/jql` with your JQL, `fields=summary,status,assignee,reporter,duedate,updated,created,comment,description,project,issuetype` and `expand=changelog`, following `nextPageToken` until `isLast` (100 per page, 20 pages max).

Each issue becomes an item:

| Item field | From |
|---|---|
| `kind` | `issue` |
| `title` | `KEY · summary` |
| `summary` | `Status · assignee` ("In Review · you", "To Do · Unassigned") |
| `url` | `<base_url>/browse/KEY` |
| `author` | reporter (Jira `accountId` handle) |
| `owner` | `you` when assigned to you, else the assignee's name |
| `due_at` | `duedate`, end of that day |
| `status` | `done` when the status category is Done, else `open` |
| `msg_count` | comment total |
| `body` | description, as plain text |
| expanded messages | last 5 comments |

**Cursor:** the newest `updated` timestamp seen (UTC, milliseconds). JQL dates are
minute-precision and read in your Jira profile's time zone, so the query reaches one
day before the cursor; re-seen issues are simply updated.

**New signal (`bump`)** on an issue Helm already has, when after the cursor:
- someone else changed its **status or assignee** (from the changelog), or
- someone else **@mentioned you** in a new comment.

Your own changes (including writes sent from Helm) never resurface an item.

**Mentions:** JQL can't reliably find issues that mention you (`comment ~ currentUser()`
is unsupported in Jira Cloud and gives inconsistent results), so the default JQL
doesn't try. Mentions on issues you're assigned to, reported or watch do bump them.
If you want to experiment, add `OR comment ~ currentUser()` to `jql`.

## What is written (outbox ops)

| Op | Payload | Request | Receipt |
|---|---|---|---|
| `jira.transition` | `{key, transition_name}` or `{key, transition_id}` | `GET …/issue/KEY/transitions`, then `POST …/issue/KEY/transitions {transition: {id}}` | `{key, transition, to, url}` |
| `jira.comment` | `{key, body}` | `POST …/issue/KEY/comment` with the text as ADF (blank line = new paragraph, newline = line break) | `{key, comment_id, url}` |
| `jira.assign` | `{key, account_id}` | `PUT …/issue/KEY/assignee {accountId}` | `{key, account_id, url}` |
| `jira.create_issue` | `{summary, assignee?, account_id?, notes_line_id, item_id}` (enqueued by Notes) | optional `GET …/user/assignable/search?project=…&query=<assignee>`, then `POST /rest/api/3/issue` | `{key, id, url, assignee, assignee_unresolved}` |

`transition_name` matches the transition's name ("Start review") or its target status
("In Review"), case-insensitive; an unknown name fails with the list of available ones.
For `create_issue`, the Notes delegate (`@Kim`) becomes the assignee only if exactly one
assignable user matches; otherwise the issue is created unassigned and the receipt names
the unresolved delegate. The summary is always your own line, verbatim.

Errors show Jira's `errorMessages` and field `errors` (e.g. `Jira 400: issuetype: Specify a valid issue type`).
