//! GitHub connector (REST). Token: the `github_token` secret (a fine-grained
//! personal access token), else `gh auth token` from the GitHub CLI.
//!
//! Poll uses the issue search API rather than notifications: search can be
//! scoped to repos/orgs, filtered by `updated:>=cursor`, and answers "review
//! requested from me" directly. Notifications would need a thread-per-item
//! fetch and depend on the user's watch settings. Search is limited to 30
//! requests/minute; one poll makes three searches (plus pages), well below that.
//! The three queries are split by `is:issue` / `is:pr` because GitHub rejects
//! mixed issue+PR searches from app-style tokens with 422.
//!
//! Per changed item we make at most two core-API calls: the PR itself (files
//! changed, requested reviewers) and, when it has comments and changed since
//! the cursor, its comments since the cursor (for bump and expanded context).
//!
//! After `apply`, `link_repo_topics` files items from a repo that is linked to
//! exactly one topic as a *suggested* link to that topic.

use crate::sync::http::Response;
use crate::sync::{local_ts, Batch, Connector, Ctx, Handle, ItemUpsert, Message};
use chrono::{DateTime, Utc};
use rusqlite::Connection;
use serde_json::{json, Value};

pub struct GitHub;

/// Default secret name for the token; `config.secret` overrides it.
pub const DEFAULT_SECRET: &str = "github_token";
const API: &str = "https://api.github.com";
/// Pinned so response shapes don't shift under us. 2026-03-10 is the newest
/// version; none of its breaking changes touch the endpoints used here.
const API_VERSION: &str = "2026-03-10";
const PER_PAGE: usize = 100;
/// Search returns at most 1000 results per query.
const MAX_PAGES: usize = 10;
const DEFAULT_LOOKBACK_DAYS: i64 = 14;

/// What running `gh` returned. Only `stderr` may ever be shown: `stdout` is the token.
pub struct GhOutput {
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
}

/// Where `gh` usually lives: PATH, Homebrew, Nix (user profile, nix-darwin
/// per-user and system profiles), then whatever the login shell resolves.
fn gh_candidates() -> Vec<String> {
    let home = std::env::var("HOME").unwrap_or_default();
    let user = std::env::var("USER").unwrap_or_default();
    let mut c: Vec<String> = vec![
        "gh".into(),
        "/opt/homebrew/bin/gh".into(),
        "/usr/local/bin/gh".into(),
        format!("{home}/.nix-profile/bin/gh"),
        format!("/etc/profiles/per-user/{user}/bin/gh"),
        "/run/current-system/sw/bin/gh".into(),
        "/nix/var/nix/profiles/default/bin/gh".into(),
        "/home/linuxbrew/.linuxbrew/bin/gh".into(),
        "/usr/bin/gh".into(),
    ];
    // Last resort: ask the login shell, which sees the user's full PATH.
    if let Ok(shell) = std::env::var("SHELL") {
        if let Ok(o) = std::process::Command::new(shell).args(["-lc", "command -v gh"]).stdin(std::process::Stdio::null()).output() {
            let p = String::from_utf8_lossy(&o.stdout).trim().to_string();
            if o.status.success() && p.starts_with('/') {
                c.push(p);
            }
        }
    }
    c
}

/// Runs `gh` with these arguments. Swapped out in tests; no real process there.
pub type GhRunner<'r> = dyn Fn(&[&str]) -> std::io::Result<GhOutput> + 'r;

/// The real `gh`. Apps started from the Dock or Finder get a bare PATH
/// (`/usr/bin:/bin:…`), so the usual install locations are tried too.
/// `config.gh_path` names the binary outright.
fn run_gh(path: Option<&str>) -> impl Fn(&[&str]) -> std::io::Result<GhOutput> + '_ {
    move |args: &[&str]| {
        let candidates: Vec<String> = match path {
            Some(p) => vec![p.to_string()],
            None => gh_candidates(),
        };
        let mut last = std::io::Error::from(std::io::ErrorKind::NotFound);
        for c in candidates {
            match std::process::Command::new(&c).args(args).stdin(std::process::Stdio::null()).output() {
                Ok(o) => {
                    return Ok(GhOutput {
                        success: o.status.success(),
                        stdout: String::from_utf8_lossy(&o.stdout).into_owned(),
                        stderr: String::from_utf8_lossy(&o.stderr).into_owned(),
                    })
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => last = e,
                Err(e) => return Err(e),
            }
        }
        Err(last)
    }
}

/// `github.com` for the public API, else the host of `api_url`
/// (`https://ghe.acme.com/api/v3` → `ghe.acme.com`), as `gh` names hosts.
fn gh_hostname(base: &str) -> String {
    let host = base.split("://").nth(1).unwrap_or(base).split('/').next().unwrap_or("");
    match host {
        "api.github.com" | "" => "github.com".into(),
        h => h.to_string(),
    }
}

/// The token: the `github_token` secret (or `config.secret`), else whatever
/// `gh auth token` prints for this host (and `config.gh_user`, when gh is
/// logged in to several accounts). The token is never logged or put in an error.
fn token(ctx: &Ctx, base: &str, gh: &GhRunner<'_>) -> anyhow::Result<String> {
    let secret = ctx.source.config.get("secret").and_then(Value::as_str).unwrap_or(DEFAULT_SECRET);
    if let Some(t) = (ctx.secret)(secret).filter(|t| !t.trim().is_empty()) {
        return Ok(t);
    }
    let host = gh_hostname(base);
    let mut args = vec!["auth", "token", "--hostname", host.as_str()];
    let user = ctx.source.config.get("gh_user").and_then(Value::as_str).filter(|u| !u.is_empty());
    if let Some(u) = user {
        args.extend(["--user", u]);
    }
    let shown = args.join(" ");
    let fail = |why: String| anyhow::anyhow!("GitHub: no '{secret}' secret for source '{}', and `gh {shown}` {why}", ctx.source.name);
    let out = match gh(&args) {
        Ok(o) => o,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(fail("didn't run: the GitHub CLI isn't installed (or set config.gh_path)".into()))
        }
        Err(e) => return Err(fail(format!("didn't run: {e}"))),
    };
    let t = out.stdout.trim();
    if !out.success || t.is_empty() {
        // gh's own reason ("no oauth token found for github.com", "not logged
        // in to github.com account x"). Never stdout: that's where a token goes.
        let why: String = out.stderr.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("printed no token").chars().take(200).collect();
        let login = match user {
            Some(u) => format!("gh auth login --hostname {host} (as {u})"),
            None => format!("gh auth login --hostname {host}"),
        };
        return Err(fail(format!("failed: {why}. Run `{login}`, or store a token")));
    }
    Ok(t.to_string())
}

struct Api<'a> {
    ctx: &'a Ctx<'a>,
    base: String,
    auth: String,
}

impl<'a> Api<'a> {
    fn new(ctx: &'a Ctx<'a>, gh: &GhRunner<'_>) -> anyhow::Result<Api<'a>> {
        let base = ctx.source.config.get("api_url").and_then(Value::as_str).unwrap_or(API).trim_end_matches('/').to_string();
        let token = token(ctx, &base, gh)?;
        Ok(Api { ctx, base, auth: format!("Bearer {token}") })
    }

    fn call(&self, method: &str, path: &str, body: Option<&Value>) -> anyhow::Result<Response> {
        let url = format!("{}{path}", self.base);
        let mut headers = vec![
            ("authorization", self.auth.as_str()),
            ("accept", "application/vnd.github+json"),
            ("x-github-api-version", API_VERSION),
            ("user-agent", "alethiometer"),
        ];
        let body = body.map(Value::to_string);
        if body.is_some() {
            headers.push(("content-type", "application/json"));
        }
        let resp = self.ctx.http.request(method, &url, &headers, body.as_deref())?;
        if (200..300).contains(&resp.status) {
            Ok(resp)
        } else {
            Err(error(&resp))
        }
    }

    fn get(&self, path: &str) -> anyhow::Result<Value> {
        self.call("GET", path, None)?.json()
    }
}

/// GitHub errors are `{"message": ..., "errors": [{resource, field, code, message}]}`.
/// Explain the common auth failures; never echo the request (it carries the token).
fn error(resp: &Response) -> anyhow::Error {
    let v: Value = serde_json::from_str(&resp.body).unwrap_or(Value::Null);
    let message = v.get("message").and_then(Value::as_str).unwrap_or("").to_string();
    let details: Vec<String> = v
        .get("errors")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|e| match e {
            Value::String(s) => s.clone(),
            _ => e
                .get("message")
                .and_then(Value::as_str)
                .map(str::to_owned)
                .unwrap_or_else(|| {
                    let f = |k: &str| e.get(k).and_then(Value::as_str).unwrap_or("").to_string();
                    format!("{} {} {}", f("resource"), f("field"), f("code")).trim().to_string()
                }),
        })
        .collect();
    let hint = match resp.status {
        401 => "token rejected or expired",
        403 if message.to_lowercase().contains("rate limit") => "rate limited; will retry next poll",
        403 => "token lacks permission (check its repo access and permissions, and whether the org approved it)",
        404 => "not found, or the token can't see this repo",
        422 => "GitHub rejected the request",
        _ => "",
    };
    let mut text = [hint, message.as_str()].iter().filter(|s| !s.is_empty()).copied().collect::<Vec<_>>().join(": ");
    if !details.is_empty() {
        text.push_str(&format!(" ({})", details.join("; ")));
    }
    anyhow::anyhow!("GitHub {}: {text}", resp.status)
}

/// Query-string encoding: everything but RFC 3986 unreserved characters.
pub fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

impl Connector for GitHub {
    fn poll(&self, ctx: &Ctx) -> anyhow::Result<Batch> {
        self.poll_with(ctx, &run_gh(gh_path(ctx)))
    }

    fn send(&self, ctx: &Ctx, op: &str, payload: &Value) -> anyhow::Result<Value> {
        self.send_with(ctx, op, payload, &run_gh(gh_path(ctx)))
    }
}

fn gh_path<'c>(ctx: &'c Ctx) -> Option<&'c str> {
    ctx.source.config.get("gh_path").and_then(Value::as_str).filter(|p| !p.is_empty())
}

impl GitHub {
    /// `poll` with the `gh` runner passed in (tests pass a fake).
    pub fn poll_with(&self, ctx: &Ctx, gh: &GhRunner<'_>) -> anyhow::Result<Batch> {
        let api = Api::new(ctx, gh)?;
        let me = api.get("/user")?.get("login").and_then(Value::as_str).unwrap_or_default().to_string();
        let cursor = ctx.source.cursor.as_deref().and_then(parse_time);
        let since = cursor.unwrap_or_else(|| {
            let days = ctx.source.config.get("lookback_days").and_then(Value::as_i64).unwrap_or(DEFAULT_LOOKBACK_DAYS);
            now_utc(ctx) - chrono::Duration::days(days)
        });
        let scope = scope(ctx);

        // (search result, review requested from me, matched involves:@me for another reason)
        let mut found: Vec<(Value, bool, bool)> = vec![];
        for (role, is, review) in
            [("involves:@me", "is:issue", false), ("involves:@me", "is:pr", false), ("review-requested:@me", "is:pr", true)]
        {
            let q = format!("updated:>={} {scope}{role} {is}", since.format("%Y-%m-%dT%H:%M:%SZ"));
            for it in search(&api, &q)? {
                let url = it.get("html_url").cloned();
                match found.iter_mut().find(|(f, _, _)| f.get("html_url") == url.as_ref()) {
                    Some(f) => {
                        f.1 |= review;
                        f.2 |= !review;
                    }
                    None => found.push((it, review, !review)),
                }
            }
        }

        let team_reviews = ctx.source.config.get("team_review_requests").and_then(Value::as_bool).unwrap_or(true);
        let mut batch = Batch::default();
        let mut newest = cursor;
        for (it, review, involved) in &found {
            let Some(item) = map_item(&api, it, *review, *involved, team_reviews, &me, cursor)? else { continue };
            if let Some(u) = it.get("updated_at").and_then(Value::as_str).and_then(parse_time) {
                if newest.is_none_or(|n| u > n) {
                    newest = Some(u);
                }
            }
            batch.items.push(item);
        }
        batch.cursor = newest.map(|t| t.format("%Y-%m-%dT%H:%M:%SZ").to_string());
        Ok(batch)
    }

    /// `send` with the `gh` runner passed in (tests pass a fake).
    pub fn send_with(&self, ctx: &Ctx, op: &str, payload: &Value, gh: &GhRunner<'_>) -> anyhow::Result<Value> {
        let api = Api::new(ctx, gh)?;
        let repo = repo_arg(payload)?;
        let number = payload.get("number").and_then(Value::as_u64).filter(|n| *n > 0).ok_or_else(|| anyhow::anyhow!("payload needs a 'number'"))?;
        match op {
            "github.comment" => {
                let body = text(payload, "body")?;
                let r = api.call("POST", &format!("/repos/{repo}/issues/{number}/comments"), Some(&json!({ "body": body })))?.json()?;
                Ok(json!({ "repo": repo, "number": number, "comment_id": r.get("id"), "url": r.get("html_url") }))
            }
            "github.review_comment" => {
                // A review with a body only. Approving or requesting changes is
                // deliberately not offered from Alethiometer.
                let event = payload.get("event").and_then(Value::as_str).unwrap_or("COMMENT");
                if event != "COMMENT" {
                    anyhow::bail!("github.review_comment only sends COMMENT reviews, not {event}");
                }
                let body = text(payload, "body")?;
                let r = api
                    .call("POST", &format!("/repos/{repo}/pulls/{number}/reviews"), Some(&json!({ "body": body, "event": "COMMENT" })))?
                    .json()?;
                Ok(json!({ "repo": repo, "number": number, "review_id": r.get("id"), "state": r.get("state"), "url": r.get("html_url") }))
            }
            "github.label" => {
                let labels: Vec<String> = payload
                    .get("labels")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .map(str::trim)
                    .filter(|l| !l.is_empty())
                    .map(str::to_owned)
                    .collect();
                if labels.is_empty() {
                    anyhow::bail!("github.label needs at least one label");
                }
                // POST adds to the existing labels; it never removes any.
                let r = api.call("POST", &format!("/repos/{repo}/issues/{number}/labels"), Some(&json!({ "labels": labels })))?.json()?;
                let now: Vec<&str> = r.as_array().into_iter().flatten().filter_map(|l| l.get("name").and_then(Value::as_str)).collect();
                Ok(json!({
                    "repo": repo, "number": number, "added": labels, "labels": now,
                    "url": format!("https://github.com/{repo}/issues/{number}"),
                }))
            }
            _ => anyhow::bail!("GitHub doesn't support '{op}'"),
        }
    }
}

/// `repo:a/b repo:c/d ` when `config.repos` is set, else `org:x ` per `config.orgs`.
fn scope(ctx: &Ctx) -> String {
    let list = |k: &str| -> Vec<String> {
        ctx.source.config.get(k).and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).map(str::to_owned).collect()
    };
    let repos = list("repos");
    let (qual, names) = if repos.is_empty() { ("org", list("orgs")) } else { ("repo", repos) };
    names.iter().map(|n| format!("{qual}:{n} ")).collect()
}

fn search(api: &Api, q: &str) -> anyhow::Result<Vec<Value>> {
    let mut out = vec![];
    for page in 1..=MAX_PAGES {
        let v = api.get(&format!("/search/issues?q={}&sort=updated&order=asc&per_page={PER_PAGE}&page={page}", urlencode(q)))?;
        let items = v.get("items").and_then(Value::as_array).cloned().unwrap_or_default();
        let total = v.get("total_count").and_then(Value::as_u64).unwrap_or(0) as usize;
        let empty = items.is_empty();
        out.extend(items);
        if empty || out.len() >= total {
            break;
        }
    }
    Ok(out)
}

fn map_item(api: &Api, it: &Value, review_hit: bool, involved: bool, team_reviews: bool, me: &str, cursor: Option<DateTime<Utc>>) -> anyhow::Result<Option<ItemUpsert>> {
    let s = |p: &str| it.pointer(p).and_then(Value::as_str);
    let (Some(repo), Some(number)) = (
        s("/repository_url").and_then(|u| u.split("/repos/").nth(1)).map(str::to_owned),
        it.get("number").and_then(Value::as_u64),
    ) else {
        return Ok(None);
    };
    let is_pr = it.get("pull_request").is_some_and(|p| !p.is_null());
    let updated = s("/updated_at").and_then(parse_time);
    let changed_since = |t: Option<DateTime<Utc>>| matches!((t, cursor), (Some(t), Some(c)) if t > c);
    let closed = s("/state") == Some("closed");
    let author = s("/user/login").unwrap_or("");
    let assignees: Vec<&str> = it.get("assignees").and_then(Value::as_array).into_iter().flatten().filter_map(|a| a.get("login")?.as_str()).collect();
    let assigned_to_me = assignees.contains(&me);

    let (mut review, mut label) = (review_hit, String::new());
    if is_pr {
        let pr = api.get(&format!("/repos/{repo}/pulls/{number}"))?;
        let individually_requested = pr.get("requested_reviewers").and_then(Value::as_array).into_iter().flatten().any(|r| r.get("login").and_then(Value::as_str) == Some(me));
        review |= individually_requested;
        if review_hit && !involved && !individually_requested && !team_reviews {
            return Ok(None);
        }
        let merged = pr.get("merged").and_then(Value::as_bool).unwrap_or(false) || it.pointer("/pull_request/merged_at").is_some_and(|m| !m.is_null());
        let state = if merged {
            "Merged"
        } else if closed {
            "Closed"
        } else if review {
            "Review requested"
        } else if pr.get("draft").and_then(Value::as_bool).unwrap_or(false) {
            "Draft"
        } else {
            "Open"
        };
        let files = pr.get("changed_files").and_then(Value::as_u64).unwrap_or(0);
        label = format!("{state} · {files} file{}", if files == 1 { "" } else { "s" });
    } else {
        label.push_str(if closed { "Closed" } else { "Open" });
        if assigned_to_me {
            label.push_str(" · assigned to you");
        }
    }

    // New comments since the cursor: for bump and as expanded context.
    let comment_count = it.get("comments").and_then(Value::as_i64).unwrap_or(0);
    let mut messages = vec![];
    let mut others_commented = false;
    if let (Some(c), true) = (cursor, comment_count > 0 && changed_since(updated)) {
        let since = urlencode(&c.format("%Y-%m-%dT%H:%M:%SZ").to_string());
        let comments = api.get(&format!("/repos/{repo}/issues/{number}/comments?since={since}&per_page={PER_PAGE}"))?;
        for cm in comments.as_array().into_iter().flatten() {
            let login = cm.pointer("/user/login").and_then(Value::as_str).unwrap_or("?");
            let created = cm.get("created_at").and_then(Value::as_str).and_then(parse_time);
            if changed_since(created) && login != me {
                others_commented = true;
            }
            if let Some(at) = cm.get("created_at").and_then(Value::as_str).and_then(local_ts) {
                messages.push(Message {
                    at,
                    author: login.to_string(),
                    text: cm.get("body").and_then(Value::as_str).unwrap_or("").to_string(),
                    author_id: (!login.is_empty() && login != "?").then(|| login.to_string()),
                    ..Default::default()
                });
            }
        }
    }
    // New signal: closed/merged since the cursor, someone else commented, or a
    // PR awaiting your review moved (new request, new commits, re-request).
    let closed_since = changed_since(s("/closed_at").and_then(parse_time));
    let bump = cursor.is_some() && (closed_since || others_commented || (review && changed_since(updated)));

    let owner = if assigned_to_me || review || (is_pr && author == me) {
        Some("you".to_string())
    } else {
        assignees.first().map(|a| a.to_string())
    };
    let Some(occurred_at) = s("/updated_at").and_then(local_ts) else { return Ok(None) };
    Ok(Some(ItemUpsert {
        external_id: format!("{repo}#{number}"),
        kind: if is_pr { "pr" } else { "issue" }.into(),
        title: format!("{repo}#{number} · {}", s("/title").unwrap_or("")),
        body: s("/body").unwrap_or("").to_string(),
        summary: Some(label),
        url: s("/html_url").map(str::to_owned),
        author: (!author.is_empty()).then(|| Handle { handle: author.to_string(), display_name: author.to_string() }),
        occurred_at,
        ended_at: None,
        due_at: None,
        status: if closed { "done" } else { "open" }.into(),
        msg_count: Some(comment_count),
        owner,
        raw_json: it.clone(),
        messages,
        event: None,
        bump,
        drop_event: false,
        attach_to: None,
    }))
}

/// Suggest a topic for GitHub items whose repo is linked (`topic_repos`) to
/// exactly one active topic. Only items with no topic link at all are touched,
/// so a confirmed, wrong or other suggested link is never overridden, and
/// nothing is ever confirmed automatically. Runs after `apply` for every sync,
/// so linking a repo later also files items already synced.
pub fn link_repo_topics(conn: &Connection, source_id: i64) -> crate::error::Result<usize> {
    let n = conn.execute(
        "WITH repo_topic AS (
           SELECT lower(tr.repo) AS repo, min(tr.topic_id) AS topic_id, min(tr.repo) AS name
             FROM topic_repos tr JOIN topics t ON t.id = tr.topic_id
            WHERE t.archived_at IS NULL
            GROUP BY lower(tr.repo) HAVING count(DISTINCT tr.topic_id) = 1)
         INSERT INTO topic_links (item_id, topic_id, state, confidence, reason)
         SELECT i.id, rt.topic_id, 'suggested', 0.9, 'linked repo ' || rt.name
           FROM items i JOIN repo_topic rt ON rt.repo = lower(substr(i.external_id, 1, instr(i.external_id, '#') - 1))
          WHERE i.source_id = ?1
            AND NOT EXISTS (SELECT 1 FROM topic_links l WHERE l.item_id = i.id)",
        [source_id],
    )?;
    Ok(n)
}

fn repo_arg(payload: &Value) -> anyhow::Result<String> {
    let repo = text(payload, "repo")?;
    let ok = repo.split_once('/').is_some_and(|(o, r)| {
        let valid = |s: &str| !s.is_empty() && s != "." && s != ".." && s.chars().all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c));
        valid(o) && valid(r)
    });
    if !ok {
        anyhow::bail!("'{repo}' isn't an owner/repo name");
    }
    Ok(repo)
}

fn text(payload: &Value, field: &str) -> anyhow::Result<String> {
    payload
        .get(field)
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .map(str::to_owned)
        .ok_or_else(|| anyhow::anyhow!("payload needs a non-empty '{field}'"))
}

fn parse_time(s: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(s).ok().map(|t| t.with_timezone(&Utc))
}

fn now_utc(ctx: &Ctx) -> DateTime<Utc> {
    ctx.now.and_local_timezone(chrono::Local).earliest().map(|t| t.with_timezone(&Utc)).unwrap_or_else(Utc::now)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync::http::FakeHttp;
    use crate::sync::Source;

    const USER: &str = include_str!("../../../tests/fixtures/github/user.json");
    const ISSUES: &str = include_str!("../../../tests/fixtures/github/search_issues.json");
    const PRS_1: &str = include_str!("../../../tests/fixtures/github/search_prs_page1.json");
    const PRS_2: &str = include_str!("../../../tests/fixtures/github/search_prs_page2.json");
    const REVIEW: &str = include_str!("../../../tests/fixtures/github/search_review_requested.json");
    const PR_482: &str = include_str!("../../../tests/fixtures/github/pull_482.json");
    const PR_41: &str = include_str!("../../../tests/fixtures/github/pull_41.json");
    const PR_77: &str = include_str!("../../../tests/fixtures/github/pull_77.json");
    const COMMENTS_482: &str = include_str!("../../../tests/fixtures/github/comments_482.json");
    const COMMENTS_9: &str = include_str!("../../../tests/fixtures/github/comments_9.json");
    const EMPTY: &str = r#"{"total_count":0,"incomplete_results":false,"items":[]}"#;

    fn source(cursor: Option<&str>, config: Value) -> Source {
        Source { id: 1, kind: "github".into(), tag: "work".into(), name: "GitHub".into(), config, cursor: cursor.map(str::to_owned) }
    }

    fn with_ctx<T>(src: &Source, http: &FakeHttp, f: impl FnOnce(&Ctx) -> T) -> T {
        let secret = |k: &str| (k == DEFAULT_SECRET).then(|| "github_pat_secret".to_string());
        let now = chrono::NaiveDate::from_ymd_opt(2026, 9, 27).unwrap().and_hms_opt(10, 0, 0).unwrap();
        f(&Ctx { source: src, http, secret: &secret, now })
    }

    /// With the secret stored, `gh` must never run.
    fn no_gh(_: &[&str]) -> std::io::Result<GhOutput> {
        panic!("gh must not run when the secret is set")
    }

    impl GitHub {
        fn poll_t(&self, ctx: &Ctx) -> anyhow::Result<Batch> {
            self.poll_with(ctx, &no_gh)
        }
    }

    fn fake() -> FakeHttp {
        let http = FakeHttp::default();
        http.on("GET", "/user", 200, USER);
        http.on("GET", "/search/issues", 200, EMPTY);
        http.on("GET", "involves%3A%40me%20is%3Aissue&", 200, ISSUES);
        http.on("GET", "involves%3A%40me%20is%3Apr&", 200, PRS_1);
        http.on("GET", "involves%3A%40me%20is%3Apr&sort=updated&order=asc&per_page=100&page=2", 200, PRS_2);
        http.on("GET", "review-requested%3A%40me%20is%3Apr&", 200, REVIEW);
        http.on("GET", "/repos/platform/ingest/pulls/482", 200, PR_482);
        http.on("GET", "/repos/platform/rfcs/pulls/41", 200, PR_41);
        http.on("GET", "/repos/platform/ingest/pulls/77", 200, PR_77);
        http.on("GET", "/repos/platform/ingest/issues/482/comments", 200, COMMENTS_482);
        http.on("GET", "/repos/platform/observability-config/issues/9/comments", 200, COMMENTS_9);
        http
    }

    fn poll(cursor: Option<&str>, config: Value) -> (Batch, Vec<String>) {
        let http = fake();
        let src = source(cursor, config);
        let b = with_ctx(&src, &http, |ctx| GitHub.poll_t(ctx)).unwrap();
        (b, http.requests().into_iter().map(|r| r.1).collect())
    }

    #[test]
    fn team_only_review_requests_are_filtered_out_when_disabled() {
        let http = FakeHttp::default();
        http.on("GET", "/user", 200, USER);
        http.on("GET", "/search/issues", 200, EMPTY);
        http.on("GET", "review-requested%3A%40me%20is%3Apr&", 200, r#"{"total_count":1,"incomplete_results":false,"items":[{
            "url": "https://api.github.com/repos/platform/ingest/issues/90",
            "repository_url": "https://api.github.com/repos/platform/ingest",
            "html_url": "https://github.com/platform/ingest/pull/90",
            "number": 90, "title": "Team-requested PR", "user": {"login": "kimlee"}, "assignees": [],
            "state": "open", "comments": 0, "created_at": "2026-09-20T10:00:00Z", "updated_at": "2026-09-27T07:40:00Z",
            "closed_at": null, "draft": false, "pull_request": {"merged_at": null}
        }]}"#);
        http.on(
            "GET",
            "/repos/platform/ingest/pulls/90",
            200,
            r#"{"requested_reviewers": [], "requested_teams": [{"slug": "platform-team"}], "merged": false, "draft": false, "changed_files": 2}"#,
        );
        let src = source(None, json!({ "team_review_requests": false }));
        let b = with_ctx(&src, &http, |ctx| GitHub.poll_t(ctx)).unwrap();
        assert!(b.items.is_empty(), "{:#?}", b.items);

        let src = source(None, json!({}));
        let b = with_ctx(&src, &http, |ctx| GitHub.poll_t(ctx)).unwrap();
        assert_eq!(b.items.len(), 1);
        assert_eq!(b.items[0].owner.as_deref(), Some("you"));
    }

    #[test]
    fn polls_three_searches_with_paging_and_maps() {
        let (b, urls) = poll(None, json!({}));
        let searches: Vec<&String> = urls.iter().filter(|u| u.contains("/search/issues")).collect();
        assert_eq!(searches.len(), 4, "{searches:#?}");
        assert!(searches[0].starts_with("https://api.github.com/search/issues?q=updated%3A%3E%3D2026-09-"));
        assert!(searches[2].ends_with("&page=2"));
        // Merged by URL: #482 came from both involves and review-requested.
        let ids: Vec<&str> = b.items.iter().map(|i| i.external_id.as_str()).collect();
        assert_eq!(ids, ["platform/observability-config#9", "platform/ingest#482", "platform/rfcs#41", "platform/ingest#77"]);
        // First sync: no comment fetches, no bumps.
        assert!(!urls.iter().any(|u| u.contains("/comments")));
        assert!(b.items.iter().all(|i| !i.bump));

        let pr = &b.items[1];
        assert_eq!(pr.kind, "pr");
        assert_eq!(pr.title, "platform/ingest#482 · Cap retries on 5xx");
        assert_eq!(pr.summary.as_deref(), Some("Review requested · 3 files"));
        assert_eq!(pr.url.as_deref(), Some("https://github.com/platform/ingest/pull/482"));
        assert_eq!(pr.author.as_ref().unwrap().handle, "jpark");
        assert_eq!(pr.owner.as_deref(), Some("you"));
        assert_eq!(pr.status, "open");
        assert_eq!(pr.msg_count, Some(4));
        assert_eq!(pr.occurred_at, local_ts("2026-09-27T07:40:00Z").unwrap());

        let issue = &b.items[0];
        assert_eq!(issue.kind, "issue");
        assert_eq!(issue.summary.as_deref(), Some("Open · assigned to you"));
        assert_eq!(issue.owner.as_deref(), Some("you"));

        let mine = &b.items[2];
        assert_eq!(mine.summary.as_deref(), Some("Open · 1 file"));
        assert_eq!(mine.owner.as_deref(), Some("you"), "authored by me");

        let merged = &b.items[3];
        assert_eq!(merged.summary.as_deref(), Some("Merged · 12 files"));
        assert_eq!(merged.status, "done");
        assert_eq!(merged.owner, None);

        assert_eq!(b.cursor.as_deref(), Some("2026-09-27T07:40:00Z"));
    }

    #[test]
    fn scope_and_cursor_go_into_the_query() {
        let (_, urls) = poll(Some("2026-09-26T12:00:00Z"), json!({ "repos": ["platform/ingest", "platform/rfcs"], "orgs": ["ignored"] }));
        let q = urlencode("updated:>=2026-09-26T12:00:00Z repo:platform/ingest repo:platform/rfcs involves:@me is:issue");
        assert!(urls.iter().any(|u| u.contains(&q)), "{urls:#?}");
        let (_, urls) = poll(None, json!({ "orgs": ["platform"] }));
        assert!(urls.iter().any(|u| u.contains(&urlencode(" org:platform review-requested:@me is:pr"))));
    }

    #[test]
    fn bump_on_new_comments_by_others_state_change_or_review_request() {
        let (b, urls) = poll(Some("2026-09-26T12:00:00Z"), json!({}));
        let get = |id: &str| b.items.iter().find(|i| i.external_id == id).unwrap();
        // #482: review requested from me and updated since; also Kim commented.
        assert!(get("platform/ingest#482").bump);
        assert_eq!(get("platform/ingest#482").messages.len(), 2);
        assert!(urls.iter().any(|u| u.contains("/repos/platform/ingest/issues/482/comments?since=2026-09-26T12%3A00%3A00Z")));
        // #9: the only comment since the cursor is mine.
        assert!(!get("platform/observability-config#9").bump);
        // #77: merged (closed_at) after the cursor.
        assert!(get("platform/ingest#77").bump);
        // #41: my PR, updated since but no comments and still open.
        assert!(!get("platform/rfcs#41").bump);
        // Cursor never goes backwards; newest updated_at wins.
        assert_eq!(b.cursor.as_deref(), Some("2026-09-27T07:40:00Z"));
        let (b, _) = poll(Some("2026-09-28T00:00:00Z"), json!({}));
        assert!(b.items.iter().all(|i| !i.bump));
        assert_eq!(b.cursor.as_deref(), Some("2026-09-28T00:00:00Z"));
    }

    #[test]
    fn auth_errors_are_explained_without_the_token() {
        let http = FakeHttp::default();
        let src = source(None, json!({}));
        http.on("GET", "/user", 401, r#"{"message":"Bad credentials","documentation_url":"https://docs.github.com/rest","status":"401"}"#);
        let err = with_ctx(&src, &http, |ctx| GitHub.poll_t(ctx)).unwrap_err().to_string();
        assert_eq!(err, "GitHub 401: token rejected or expired: Bad credentials");
        assert!(!err.contains("github_pat_secret"));
        http.on("GET", "/user", 200, USER);
        http.on("GET", "/search/issues", 403, r#"{"message":"API rate limit exceeded for user ID 1."}"#);
        let err = with_ctx(&src, &http, |ctx| GitHub.poll_t(ctx)).unwrap_err().to_string();
        assert!(err.starts_with("GitHub 403: rate limited"), "{err}");
        http.on(
            "GET",
            "/search/issues",
            403,
            r#"{"message":"Although you appear to have the correct authorization credentials, the `platform` organization has an IP allow list enabled."}"#,
        );
        let err = with_ctx(&src, &http, |ctx| GitHub.poll_t(ctx)).unwrap_err().to_string();
        assert!(err.starts_with("GitHub 403: token lacks permission"), "{err}");
    }

    fn send(op: &str, payload: Value, http: &FakeHttp) -> anyhow::Result<Value> {
        let src = source(None, json!({}));
        with_ctx(&src, http, |ctx| GitHub.send_with(ctx, op, &payload, &no_gh))
    }

    type GhCalls = std::sync::Mutex<Vec<Vec<String>>>;

    /// A fake `gh` that records its arguments and answers with `out`.
    fn fake_gh<'a>(calls: &'a GhCalls, out: impl Fn() -> std::io::Result<GhOutput> + 'a) -> impl Fn(&[&str]) -> std::io::Result<GhOutput> + 'a {
        move |args: &[&str]| {
            calls.lock().unwrap().push(args.iter().map(|a| a.to_string()).collect());
            out()
        }
    }

    fn gh_ok() -> std::io::Result<GhOutput> {
        Ok(GhOutput { success: true, stdout: "gho_fromcli123\n".into(), stderr: String::new() })
    }

    fn poll_no_secret(config: Value, gh: &GhRunner<'_>) -> (anyhow::Result<Batch>, Vec<String>) {
        let http = fake();
        let src = source(None, config);
        let secret = |_: &str| None;
        let now = chrono::NaiveDate::from_ymd_opt(2026, 9, 27).unwrap().and_hms_opt(10, 0, 0).unwrap();
        let r = GitHub.poll_with(&Ctx { source: &src, http: &http, secret: &secret, now }, gh);
        (r, http.requests().into_iter().map(|r| r.1).collect())
    }

    #[test]
    fn token_falls_back_to_the_gh_cli() {
        let calls = GhCalls::default();
        let (b, urls) = poll_no_secret(json!({}), &fake_gh(&calls, gh_ok));
        assert_eq!(b.unwrap().items.len(), 4);
        assert_eq!(urls[0], "https://api.github.com/user");
        assert_eq!(calls.lock().unwrap().clone(), vec![vec!["auth", "token", "--hostname", "github.com"]]);

        // Enterprise host from api_url; a specific account when gh has several.
        let calls = GhCalls::default();
        let cfg = json!({ "api_url": "https://ghe.acme.com/api/v3/", "gh_user": "sam-acme" });
        let (_, urls) = poll_no_secret(cfg, &fake_gh(&calls, gh_ok));
        assert_eq!(urls[0], "https://ghe.acme.com/api/v3/user");
        assert_eq!(calls.lock().unwrap()[0], ["auth", "token", "--hostname", "ghe.acme.com", "--user", "sam-acme"]);
    }

    #[test]
    fn gh_fallback_errors_are_clear_and_never_carry_the_token() {
        let calls = GhCalls::default();
        let missing = || Err(std::io::Error::from(std::io::ErrorKind::NotFound));
        let (r, urls) = poll_no_secret(json!({}), &fake_gh(&calls, missing));
        let err = r.unwrap_err().to_string();
        assert!(err.contains("no 'github_token' secret for source 'GitHub'"), "{err}");
        assert!(err.contains("GitHub CLI isn't installed"), "{err}");
        assert!(urls.is_empty(), "no request without a token");

        let logged_out = || {
            Ok(GhOutput {
                success: false,
                stdout: "gho_should_never_show".into(),
                stderr: "no oauth token found for github.com\n".into(),
            })
        };
        let (r, _) = poll_no_secret(json!({ "gh_user": "sam-acme" }), &fake_gh(&calls, logged_out));
        let err = r.unwrap_err().to_string();
        assert!(err.contains("`gh auth token --hostname github.com --user sam-acme` failed: no oauth token found for github.com"), "{err}");
        assert!(err.contains("gh auth login --hostname github.com (as sam-acme)"), "{err}");
        assert!(!err.contains("gho_should_never_show"));

        // Exit 0 but nothing printed.
        let empty = || Ok(GhOutput { success: true, stdout: "\n".into(), stderr: String::new() });
        let (r, _) = poll_no_secret(json!({}), &fake_gh(&calls, empty));
        assert!(r.unwrap_err().to_string().contains("printed no token"));
    }

    #[test]
    fn gh_hostnames() {
        assert_eq!(gh_hostname("https://api.github.com"), "github.com");
        assert_eq!(gh_hostname("https://ghe.acme.com/api/v3"), "ghe.acme.com");
        assert_eq!(gh_hostname("https://ghe.acme.com:8443/api/v3"), "ghe.acme.com:8443");
    }

    fn last_body(http: &FakeHttp) -> Value {
        serde_json::from_str(http.requests().last().unwrap().2.as_deref().unwrap()).unwrap()
    }

    #[test]
    fn comment_posts_to_issue_comments() {
        let http = FakeHttp::default();
        http.on(
            "POST",
            "/repos/platform/ingest/issues/482/comments",
            201,
            r#"{"id":1001,"html_url":"https://github.com/platform/ingest/pull/482#issuecomment-1001","body":"On it"}"#,
        );
        let r = send("github.comment", json!({ "repo": "platform/ingest", "number": 482, "body": "On it" }), &http).unwrap();
        assert_eq!(r["url"], "https://github.com/platform/ingest/pull/482#issuecomment-1001");
        let req = http.requests().pop().unwrap();
        assert_eq!((req.0.as_str(), req.1.as_str()), ("POST", "https://api.github.com/repos/platform/ingest/issues/482/comments"));
        assert_eq!(last_body(&http), json!({ "body": "On it" }));
        assert!(send("github.comment", json!({ "repo": "../x", "number": 1, "body": "hi" }), &http).is_err());
        assert!(send("github.comment", json!({ "repo": "platform/ingest", "number": 482, "body": "" }), &http).is_err());
    }

    #[test]
    fn review_comment_posts_a_comment_review() {
        let http = FakeHttp::default();
        http.on(
            "POST",
            "/repos/platform/ingest/pulls/482/reviews",
            200,
            r#"{"id":80,"state":"COMMENTED","body":"Nice","html_url":"https://github.com/platform/ingest/pull/482#pullrequestreview-80"}"#,
        );
        let r = send("github.review_comment", json!({ "repo": "platform/ingest", "number": 482, "body": "Nice", "event": "COMMENT" }), &http).unwrap();
        assert_eq!(r["state"], "COMMENTED");
        assert_eq!(r["url"], "https://github.com/platform/ingest/pull/482#pullrequestreview-80");
        assert_eq!(http.requests()[0].1, "https://api.github.com/repos/platform/ingest/pulls/482/reviews");
        assert_eq!(last_body(&http), json!({ "body": "Nice", "event": "COMMENT" }));
        let err = send("github.review_comment", json!({ "repo": "platform/ingest", "number": 482, "body": "ok", "event": "APPROVE" }), &http).unwrap_err();
        assert!(err.to_string().contains("only sends COMMENT"));
    }

    #[test]
    fn label_adds_labels_and_surfaces_422() {
        let http = FakeHttp::default();
        http.on(
            "POST",
            "/repos/platform/ingest/issues/482/labels",
            200,
            r#"[{"id":1,"name":"bug","color":"f29513","default":true},{"id":2,"name":"needs-review","color":"ededed","default":false}]"#,
        );
        let r = send("github.label", json!({ "repo": "platform/ingest", "number": 482, "labels": ["needs-review"] }), &http).unwrap();
        assert_eq!(r["labels"], json!(["bug", "needs-review"]));
        assert_eq!(last_body(&http), json!({ "labels": ["needs-review"] }));
        assert!(send("github.label", json!({ "repo": "platform/ingest", "number": 482, "labels": [] }), &http).is_err());

        http.on(
            "POST",
            "/repos/platform/ingest/issues/482/labels",
            422,
            r#"{"message":"Validation Failed","errors":[{"resource":"Label","code":"invalid","field":"name"}],"documentation_url":"https://docs.github.com/rest"}"#,
        );
        let err = send("github.label", json!({ "repo": "platform/ingest", "number": 482, "labels": ["x"] }), &http).unwrap_err();
        assert_eq!(err.to_string(), "GitHub 422: GitHub rejected the request: Validation Failed (Label name invalid)");
    }

    #[test]
    fn repo_linked_to_one_topic_gets_a_suggested_link() {
        let conn = crate::db::open_in_memory().unwrap();
        let cfg = crate::settings::SourceConfig {
            kind: "github".into(),
            tag: "work".into(),
            name: "GitHub".into(),
            enabled: true,
            poll_secs: 300,
            config: json!({}),
        };
        crate::sync::register_sources(&conn, &[cfg]).unwrap();
        let sid: i64 = conn.query_row("SELECT id FROM sources WHERE kind = 'github'", [], |r| r.get(0)).unwrap();
        let src = crate::sync::load_source(&conn, sid).unwrap();
        let (b, _) = poll(None, json!({}));
        crate::sync::apply(&conn, &src, &b).unwrap();

        let topic = |name: &str| {
            conn.execute("INSERT INTO topics (name, category) VALUES (?1, 'work')", [name]).unwrap();
            conn.last_insert_rowid()
        };
        let (dd, ingest, other) = (topic("Datadog migration"), topic("Ingest"), topic("Retries"));
        conn.execute("INSERT INTO topic_repos VALUES (?1, 'platform/observability-config')", [dd]).unwrap();
        // platform/ingest is linked to two topics: ambiguous, so no suggestion.
        conn.execute("INSERT INTO topic_repos VALUES (?1, 'platform/ingest')", [ingest]).unwrap();
        conn.execute("INSERT INTO topic_repos VALUES (?1, 'Platform/Ingest')", [other]).unwrap();
        // platform/rfcs → Ingest, but the user already marked that item wrong elsewhere.
        conn.execute("INSERT INTO topic_repos VALUES (?1, 'platform/rfcs')", [ingest]).unwrap();
        let rfc: i64 = conn.query_row("SELECT id FROM items WHERE external_id = 'platform/rfcs#41'", [], |r| r.get(0)).unwrap();
        conn.execute("INSERT INTO topic_links (item_id, topic_id, state) VALUES (?1, ?2, 'wrong')", [rfc, dd]).unwrap();

        assert_eq!(link_repo_topics(&conn, sid).unwrap(), 1);
        let links: Vec<(String, i64, String, Option<String>)> = conn
            .prepare("SELECT i.external_id, l.topic_id, l.state, l.reason FROM topic_links l JOIN items i ON i.id = l.item_id ORDER BY i.external_id")
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        assert_eq!(links.len(), 2);
        assert_eq!(links[0], ("platform/observability-config#9".into(), dd, "suggested".into(), Some("linked repo platform/observability-config".into())));
        assert_eq!(links[1].2, "wrong", "existing links are left alone");
        // Idempotent.
        assert_eq!(link_repo_topics(&conn, sid).unwrap(), 0);
    }

    #[test]
    fn urlencode_leaves_only_unreserved() {
        assert_eq!(urlencode("a b:@>=/#é~"), "a%20b%3A%40%3E%3D%2F%23%C3%A9~");
    }
}
