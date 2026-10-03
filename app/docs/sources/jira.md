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
        "due_field": "customfield_10050",
        "create_dedupe_hours": 24,
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
| `jql` | no | Which issues to follow. Default shown above. Alethiometer adds `AND updated >= "…" ORDER BY updated ASC` |
| `lookback_days` | no (14) | How far back the first sync reaches |
| `due_field` | no | Id of a custom **date-time** field that holds the real deadline (e.g. `customfield_10050`). See [Due dates](#due-dates) |
| `create_dedupe_hours` | no (24) | How far back `jira.create_issue` looks for an issue an earlier attempt already created. `0` turns the check off. See [No duplicate issues on retry](#no-duplicate-issues-on-retry) |
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
2. `GET /rest/api/3/search/jql` with your JQL, `fields=summary,status,assignee,reporter,duedate,updated,created,comment,description,project,issuetype` (plus `due_field` when set) and `expand=changelog`, following `nextPageToken` until `isLast` (100 per page, 20 pages max).

Each issue becomes an item:

| Item field | From |
|---|---|
| `kind` | `issue` |
| `title` | `KEY · summary` |
| `summary` | `Status · assignee` ("In Review · you", "To Do · Unassigned") |
| `url` | `<base_url>/browse/KEY` |
| `author` | reporter (Jira `accountId` handle) |
| `owner` | `you` when assigned to you, else the assignee's name |
| `due_at` / `due_day` | `due_field` when set on the issue (an instant, `due_at`), else `duedate` (a calendar day, `due_day`) |
| `status` | `done` when the status category is Done, else `open` |
| `msg_count` | comment total |
| `body` | description, as plain text |
| expanded messages | last 5 comments |

### Due dates

Jira's built-in **Due date** (`duedate`) is a day with no time. Alethiometer stores it as
that day, due by the end of it wherever you are, so it sorts after that day's timed work, and
shows it as a day only ("due Fri", "due Oct 2"). A due date that has a real time shows it
("due today 15:00").

If your team keeps the real deadline in a custom **Date Time Picker** field, set
`due_field` to that field's id. Alethiometer then asks for that field too, and when an issue has
it set, uses that instant (stored in UTC, shown in your local time).
Issues where the field is empty fall back to `duedate`. A custom field that holds only a
date (a *Date Picker*) is also stored as a day.

To find the field id (`customfield_` + a number), use one of these:
- Open `https://<site>.atlassian.net/rest/api/3/issue/<KEY>?expand=names` in a browser
  where you're logged in to Jira, for an issue that has the field set. The `names` object
  at the end maps ids to field names. Find your field's name there.
- Jira admins: *Settings → Issues → Custom fields*, then *⋯ → Edit details* (or
  *Contexts*) on the field. The page URL ends in `id=10050`, which means `customfield_10050`.
- `GET /rest/api/3/field` lists every field. Custom date-time fields have
  `"custom": true` and `"schema": {"type": "datetime", …}`.

**Cursor:** the newest `updated` timestamp seen (UTC, milliseconds). JQL dates are
minute-precision and read in your Jira profile's time zone, so the query reaches one
day before the cursor; re-seen issues are simply updated.

**New signal (`bump`)** on an issue Alethiometer already has, when after the cursor:
- someone else changed its **status or assignee** (from the changelog), or
- someone else **commented** (any comment, not only one that @mentions you).

Changes to other fields (priority, due date, summary, labels…) don't. Your own changes
(including writes sent from Alethiometer) never resurface an item.

**Mentions:** JQL can't reliably find issues that mention you (`comment ~ currentUser()`
is unsupported in Jira Cloud and gives inconsistent results), so the default JQL
doesn't try. Comments on issues you're assigned to, reported or watch, mentions
included, do bump them.
If you want to experiment, add `OR comment ~ currentUser()` to `jql`.

## What is written (outbox ops)

| Op | Payload | Request | Receipt |
|---|---|---|---|
| `jira.transition` | `{key, transition_name}` or `{key, transition_id}` | `GET …/issue/KEY/transitions`, then `POST …/issue/KEY/transitions {transition: {id}}` | `{key, transition, to, url}` |
| `jira.comment` | `{key, body}` | `POST …/issue/KEY/comment` with the text as ADF (blank line = new paragraph, newline = line break) | `{key, comment_id, url}` |
| `jira.assign` | `{key, account_id}` | `PUT …/issue/KEY/assignee {accountId}` | `{key, account_id, url}` |
| `jira.create_issue` | `{summary, assignee?, account_id?, notes_line_id, item_id}` (enqueued by Notes) | `GET /rest/api/3/search/jql` for an earlier attempt (see below). If none is found: optional `GET …/user/assignable/search?project=…&query=<assignee>`, then `POST /rest/api/3/issue` | `{key, id, url, assignee, assignee_unresolved, notes_line_id, existing}` |

`transition_name` matches the transition's name ("Start review") or its target status
("In Review"), case-insensitive; an unknown name fails with the list of available ones.
For `create_issue`, the Notes delegate (`@Kim`) becomes the assignee only if exactly one
assignable user matches; otherwise the issue is created unassigned and the receipt names
the unresolved delegate. The summary is always your own line, verbatim.

### No duplicate issues on retry

A create can go through on Jira's side while Alethiometer never gets the answer: a timeout, a
crash, or ⌘R on a row that had in fact worked. Retrying it blindly would file the
issue twice. So before creating, `jira.create_issue` searches:

```
project = "PLAT" AND reporter = currentUser() AND created >= -24h
  AND summary ~ "draft retry budget rfc kim" ORDER BY created DESC
```

It then takes the newest result whose summary is **exactly** the one being sent. If
there is one, that issue is the receipt (`"existing": true`) and nothing is created.

- **Why a fixed window.** The outbox row's own creation time isn't passed to the
  connector, so the lower bound is "now minus `create_dedupe_hours`" (24 by default).
  It's a relative JQL date (`-24h`), so your Jira profile's time zone doesn't matter.
- **The trade-off.** If you file the same summary twice on purpose within the window
  (in Alethiometer or in Jira, reported by you, same project), the second one returns the first
  issue instead of creating a new one. A retry more than 24 h after the first attempt
  can still duplicate. Raise `create_dedupe_hours` if you often retry failed rows days
  later, or set it to `0` to turn the check off.
- **Why `~` plus an exact compare.** `summary ~` is Jira's word search. It ignores
  punctuation, stems words ("drafting" finds "draft") and can't match exactly, so it
  only narrows the search. Alethiometer sends the summary's words only, lowercased: this keeps
  `AND`/`OR`/`NOT` from acting as operators, and drops the characters Jira's text search
  reserves. The JQL string itself is quoted with `"` and `\` escaped. If Jira still
  rejects the text query (e.g. a summary made only of stop words), Alethiometer searches
  without it. Any other search error fails the write, so an issue is never created
  without the check.
- **Search freshness.** Jira's search index can trail a create by a few seconds. Retries
  are manual (⌘R), so in practice the earlier issue is always indexed by then.

Errors show Jira's `errorMessages` and field `errors` (e.g. `Jira 400: issuetype: Specify a valid issue type`).
