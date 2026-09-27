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
        "secret": "github_token",
        "gh_user": "sam-acme",
        "read_only": true
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
| `gh_user` | no | Without a stored token: the `gh` account to take the token from, when `gh` is logged in to several (see below) |
| `gh_path` | no | Full path to the `gh` binary, if it's somewhere unusual |
| `read_only` | no (`false`) | `true`: Helm only reads from GitHub. The outbox refuses every `github.*` write for this source, so replies and labels stay in GitHub |

With neither `repos` nor `orgs`, everything the token can see is included.

## Read-only

If you only want GitHub PRs and issues to show up in Helm, set `"read_only": true`.
The outbox then refuses writes for this source before they reach the connector. This
is handy with the `gh` token below, which can write to everything your account can.
(The check lives in the outbox and applies to any source kind.)

## Token

Helm takes the token from, in order:

1. the secret **`github_token`** (or the name in `config.secret`), else
2. the **GitHub CLI**: `gh auth token --hostname <host>` (plus `--user <gh_user>`
   when set).

### Using the GitHub CLI (no secret to store)

If you already use `gh`, log in once and store nothing in Helm:

```sh
gh auth login --hostname github.com      # or your Enterprise host
gh auth status                           # shows the accounts gh knows
```

- **Host:** `github.com`, or the host of `api_url` for Enterprise Server
  (`https://ghe.acme.com/api/v3` → `--hostname ghe.acme.com`).
- **Several accounts:** `gh` can be logged in to more than one account per host, and
  otherwise hands out the *active* one. Set `gh_user` to your work login so Helm
  always gets that account, whatever `gh auth switch` last chose.
- Helm runs `gh auth token` on every poll and write, so logging in, switching or
  logging out in `gh` takes effect on the next sync.
- The token is never logged or shown. Errors name the command and quote only `gh`'s
  own message (from stderr), then say what to do, e.g. *…and `gh auth token --hostname
  github.com` failed: no oauth token found for github.com. Run `gh auth login
  --hostname github.com`, or store a token.* If `gh` isn't installed, the error says so.
- Apps started from the Dock don't see your shell's `PATH`, so Helm also looks in
  `/opt/homebrew/bin`, `/usr/local/bin`, `/home/linuxbrew/.linuxbrew/bin` and
  `/usr/bin`. Set `gh_path` if `gh` lives elsewhere.
- **Scope:** `gh`'s token is an OAuth token with `repo` and `read:org` scopes, which is
  full read *and write* access to every repo your account can reach. Helm only makes
  the requests listed on this page. Set `read_only: true` if you want that guaranteed.
- **SAML SSO orgs:** the `gh` token must be authorized for the org. `gh auth login`
  offers this. A 403 mentioning SAML means it wasn't.

### A fine-grained token instead

Store a **fine-grained personal access token** under **`github_token`** (or the name in
`config.secret`): Keychain on the Mac, `secrets.json` (mode 0600) on Linux. It limits
Helm to the repos and permissions you pick, and it wins over `gh` when both exist.

Create it at <https://github.com/settings/personal-access-tokens/new>:

- **Resource owner:** the organization (one token per org; if you have several orgs, add one source per org with its own secret name).
- **Repository access:** all repositories, or the ones you care about.
- **Repository permissions:**
  - *Issues:* **Read and write** (read issues and comments; post comments; add labels)
  - *Pull requests:* **Read and write** (read PRs; post reviews; comment/label PRs)
  - *Metadata:* **Read** (required, selected automatically)

  For a read-only source, **Read-only** on Issues and Pull requests is enough.

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
