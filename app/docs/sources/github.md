# GitHub

Kind `github`, tag `work`. Connector: `src-tauri/src/sync/connectors/github.rs`.

## settings.json

```json
{
  "sources": [
    {
      "kind": "github",
      "tag": "work",
      "name": "GitHub",
      "poll_secs": 300,
      "config": {
        "orgs": ["platform"],
        "repos": [],
        "lookback_days": 14,
        "secret": "github_token"
      }
    }
  ]
}
```

| Key | Required | Meaning |
|---|---|---|
| `repos` | no | Only these `owner/repo`s. Takes precedence over `orgs` |
| `orgs` | no | Only repos in these orgs (used when `repos` is empty) |
| `lookback_days` | no (14) | How far back the first sync reaches |
| `secret` | no (`github_token`) | Name of the secret holding the token |
| `api_url` | no | `https://api.github.com`; set for GitHub Enterprise Server (`https://host/api/v3`) |

With neither `repos` nor `orgs`, everything the token can see is included.

## Secret

Store a **fine-grained personal access token** under **`github_token`** (or the name in
`config.secret`): Keychain on the Mac, `secrets.json` (mode 0600) on Linux.

Create it at <https://github.com/settings/personal-access-tokens/new>:

- **Resource owner:** the organization (one token per org; if you have several orgs, add one source per org with its own secret name).
- **Repository access:** all repositories, or the ones you care about.
- **Repository permissions:**
  - *Issues:* **Read and write** (read issues and comments; post comments; add labels)
  - *Pull requests:* **Read and write** (read PRs; post reviews; comment/label PRs)
  - *Metadata:* **Read** (required, selected automatically)

Organizations can require owner approval for fine-grained tokens, and some block them
or enforce an expiry. If syncing says `GitHub 403: token lacks permission`, check the
token's status under the org's *Settings → Personal access tokens*. `401` means the
token expired or was revoked.

## What is read

Every poll:

1. `GET /user`, for your login.
2. Three searches (`GET /search/issues`, `sort=updated&order=asc`, 100 per page, up to 10 pages):
   - `updated:>=<cursor> <scope> involves:@me is:issue`
   - `updated:>=<cursor> <scope> involves:@me is:pr`
   - `updated:>=<cursor> <scope> review-requested:@me is:pr`

   `involves:` covers authored, assigned, mentioned and commented. Issues and PRs are
   searched separately because GitHub rejects mixed searches from app-style tokens.
3. Per PR: `GET /repos/{repo}/pulls/{n}` (files changed, requested reviewers, merged).
4. Per item with comments that changed since the cursor: `GET /repos/{repo}/issues/{n}/comments?since=<cursor>`.

Search is rate-limited to 30 requests/minute; a poll uses 3 plus extra pages.

Search rather than the notifications API: search can be scoped to repos/orgs and
filtered by update time, and answers "review requested from me" directly;
notifications depend on your watch settings and need a follow-up call per thread.

| Item field | From |
|---|---|
| `kind` | `pr` or `issue` |
| `title` | `owner/repo#123 · title` |
| `summary` | PR: `Review requested · 3 files`, `Draft · …`, `Open · …`, `Merged · …`, `Closed · …`. Issue: `Open`/`Closed`, `· assigned to you` |
| `url` | `html_url` |
| `author` | login (GitHub handle) |
| `owner` | `you` when assigned to you, your review is requested, or it's your PR; else first assignee |
| `status` | `done` when closed or merged |
| `msg_count` | issue/PR conversation comments |
| expanded messages | comments since the last sync |

**Cursor:** newest `updated_at` seen (UTC).

**New signal (`bump`)** on an item Helm already has, when after the cursor:
- it was **closed or merged**,
- **someone else commented**, or
- your **review is requested** and the PR changed (new request, new commits).

**Topics:** after each sync, an item from a repo linked to exactly one topic
(`topic_repos`) gets a **suggested** link to that topic, so it shows under that topic
in Incoming. Items that already have any topic link (confirmed, wrong or suggested) are
left alone, and nothing is confirmed automatically. Repos linked to two or more topics
aren't guessed.

## What is written (outbox ops)

| Op | Payload | Request | Receipt |
|---|---|---|---|
| `github.comment` | `{repo, number, body}` | `POST /repos/{repo}/issues/{n}/comments {body}` (works on PRs too) | `{repo, number, comment_id, url}` |
| `github.review_comment` | `{repo, number, body, event: "COMMENT"}` | `POST /repos/{repo}/pulls/{n}/reviews {body, event: "COMMENT"}` | `{repo, number, review_id, state, url}` |
| `github.label` | `{repo, number, labels: [..]}` | `POST /repos/{repo}/issues/{n}/labels {labels}` (adds; never removes) | `{repo, number, added, labels, url}` |

Bodies are always your own text. Only `COMMENT` reviews are sent; approving or
requesting changes stays in GitHub. Requests send `X-GitHub-Api-Version: 2026-03-10`.

Errors explain 401/403/404/422 and include GitHub's message and field errors, e.g.
`GitHub 422: GitHub rejected the request: Validation Failed (Label name invalid)`.
