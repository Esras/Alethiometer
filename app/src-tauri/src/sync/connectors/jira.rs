//! Jira Cloud connector (REST API v3), basic auth with email + API token.
//!
//! Poll: one JQL search (`GET /rest/api/3/search/jql`, paged by `nextPageToken`;
//! the classic `/search` endpoint is gone). The cursor is the newest `updated`
//! instant we have seen, stored as RFC 3339 UTC with milliseconds.
//!
//! JQL dates have minute precision and are read in the *Jira profile's* time
//! zone, which we don't know without a tz database. So the JQL bound is the
//! cursor minus one day (covers any UTC offset) and the overlap is harmless:
//! upserts are idempotent, and "is this new signal?" (`bump`) is decided by
//! comparing exact changelog/comment timestamps against the cursor instant.
//!
//! Send: transition, comment (plain text → ADF), assign, create_issue (from Notes).
//! `create_issue` first looks for the same issue created by an earlier attempt
//! (see `find_created`), so a retried outbox row doesn't file a duplicate.
//!
//! Due dates: Jira's built-in `duedate` is a date without a time; it's stored
//! as 23:59:59 that day and views show it as a day only. A custom
//! date-time field named by `config.due_field` wins when an issue has it set.

use super::github::urlencode;
use crate::sync::http::Response;
use crate::sync::{local_ts, Batch, Connector, Ctx, Handle, ItemUpsert, Message};
use chrono::{DateTime, FixedOffset, Utc};
use serde_json::{json, Value};

pub struct Jira;

/// Default secret name for the API token; `config.secret` overrides it.
pub const DEFAULT_SECRET: &str = "jira_api_token";

/// Mentions can't be searched reliably in JQL (`comment ~ currentUser()` is
/// unsupported and inconsistent in Jira Cloud), so the default covers what JQL
/// can express. Mentions on these issues still bump them (see `bump`).
pub const DEFAULT_JQL: &str = "assignee = currentUser() OR reporter = currentUser() OR watcher = currentUser()";

const PAGE_SIZE: usize = 100;
const MAX_PAGES: usize = 20;
const DEFAULT_LOOKBACK_DAYS: i64 = 14;
const FIELDS: &str = "summary,status,assignee,reporter,duedate,updated,created,comment,description,project,issuetype";
/// How many recent comments to keep as expanded context.
const KEEP_COMMENTS: usize = 5;
/// How far back `create_issue` looks for an issue an earlier attempt created.
const DEFAULT_DEDUPE_HOURS: i64 = 24;

struct Api<'a> {
    ctx: &'a Ctx<'a>,
    base: String,
    auth: String,
}

impl<'a> Api<'a> {
    fn new(ctx: &'a Ctx<'a>) -> anyhow::Result<Api<'a>> {
        let base = ctx.config_str("base_url")?.trim_end_matches('/').to_string();
        let email = ctx.config_str("email")?;
        let secret = ctx.source.config.get("secret").and_then(Value::as_str).unwrap_or(DEFAULT_SECRET);
        let token = ctx.require_secret(secret)?;
        Ok(Api { ctx, base, auth: format!("Basic {}", base64(format!("{email}:{token}").as_bytes())) })
    }

    fn call(&self, method: &str, path: &str, body: Option<&Value>) -> anyhow::Result<Response> {
        let url = format!("{}{path}", self.base);
        let mut headers = vec![("authorization", self.auth.as_str()), ("accept", "application/json")];
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

    fn browse(&self, key: &str) -> String {
        format!("{}/browse/{key}", self.base)
    }
}

/// Jira's error body is `{"errorMessages": [...], "errors": {field: msg}}`.
/// Surface those; never the request (it carries the token).
fn error(resp: &Response) -> anyhow::Error {
    let v: Value = serde_json::from_str(&resp.body).unwrap_or(Value::Null);
    let mut parts: Vec<String> = v
        .get("errorMessages")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).map(str::to_owned).collect())
        .unwrap_or_default();
    if let Some(errs) = v.get("errors").and_then(Value::as_object) {
        parts.extend(errs.iter().map(|(k, m)| format!("{k}: {}", m.as_str().unwrap_or(&m.to_string()))));
    }
    let detail = if !parts.is_empty() {
        parts.join("; ")
    } else {
        match resp.status {
            401 => "email or API token rejected".into(),
            403 => "no permission (or CAPTCHA required; log in once in the browser)".into(),
            429 => "rate limited; will retry next poll".into(),
            _ => resp.body.chars().take(200).collect(),
        }
    };
    anyhow::anyhow!("Jira {}: {detail}", resp.status)
}

impl Connector for Jira {
    fn poll(&self, ctx: &Ctx) -> anyhow::Result<Batch> {
        let api = Api::new(ctx)?;
        let me = api.get("/rest/api/3/myself")?;
        let me = me.get("accountId").and_then(Value::as_str).unwrap_or_default().to_string();

        let cursor = ctx.source.cursor.as_deref().and_then(parse_time).map(|t| t.with_timezone(&Utc));
        let since = cursor.unwrap_or_else(|| {
            let days = ctx.source.config.get("lookback_days").and_then(Value::as_i64).unwrap_or(DEFAULT_LOOKBACK_DAYS);
            now_utc(ctx) - chrono::Duration::days(days)
        });
        let bound = (since - chrono::Duration::days(1)).format("%Y-%m-%d %H:%M");
        let base_jql = ctx.source.config.get("jql").and_then(Value::as_str).unwrap_or(DEFAULT_JQL);
        let jql = format!("({base_jql}) AND updated >= \"{bound}\" ORDER BY updated ASC");

        let due_field = due_field(ctx)?;
        let fields = match due_field {
            Some(f) => format!("{FIELDS},{f}"),
            None => FIELDS.to_string(),
        };

        let mut batch = Batch::default();
        let mut newest = cursor;
        let mut token: Option<String> = None;
        for _ in 0..MAX_PAGES {
            let mut path = format!(
                "/rest/api/3/search/jql?jql={}&maxResults={PAGE_SIZE}&fields={fields}&expand=changelog",
                urlencode(&jql)
            );
            if let Some(t) = &token {
                path.push_str(&format!("&nextPageToken={}", urlencode(t)));
            }
            let page = api.get(&path)?;
            for issue in page.get("issues").and_then(Value::as_array).into_iter().flatten() {
                let Some(item) = map_issue(&api, issue, &me, cursor, due_field) else { continue };
                if let Some(u) = issue.pointer("/fields/updated").and_then(Value::as_str).and_then(parse_time) {
                    let u = u.with_timezone(&Utc);
                    if newest.is_none_or(|n| u > n) {
                        newest = Some(u);
                    }
                }
                batch.items.push(item);
            }
            token = page.get("nextPageToken").and_then(Value::as_str).map(str::to_owned);
            let last = page.get("isLast").and_then(Value::as_bool).unwrap_or(token.is_none());
            if last || token.is_none() {
                break;
            }
        }
        batch.cursor = newest.map(|t| t.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string());
        Ok(batch)
    }

    fn send(&self, ctx: &Ctx, op: &str, payload: &Value) -> anyhow::Result<Value> {
        let api = Api::new(ctx)?;
        match op {
            "jira.transition" => transition(&api, payload),
            "jira.comment" => {
                let key = issue_key(payload)?;
                let body = text(payload, "body")?;
                let resp = api.call("POST", &format!("/rest/api/3/issue/{key}/comment"), Some(&json!({ "body": to_adf(&body) })))?;
                let id = resp.json()?.get("id").and_then(Value::as_str).unwrap_or_default().to_string();
                Ok(json!({ "key": key, "comment_id": id, "url": format!("{}?focusedCommentId={id}", api.browse(&key)) }))
            }
            "jira.assign" => {
                let key = issue_key(payload)?;
                let account = text(payload, "account_id")?;
                api.call("PUT", &format!("/rest/api/3/issue/{key}/assignee"), Some(&json!({ "accountId": account })))?;
                Ok(json!({ "key": key, "account_id": account, "url": api.browse(&key) }))
            }
            "jira.create_issue" => create_issue(&api, payload),
            _ => anyhow::bail!("Jira doesn't support '{op}'"),
        }
    }
}

/// `config.due_field`: a field id like `customfield_10050`. Checked because it
/// goes into the `fields` query parameter as-is.
fn due_field<'c>(ctx: &'c Ctx) -> anyhow::Result<Option<&'c str>> {
    let Some(f) = ctx.source.config.get("due_field").and_then(Value::as_str).map(str::trim).filter(|f| !f.is_empty()) else {
        return Ok(None);
    };
    if !f.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        anyhow::bail!("config.due_field should be a field id like customfield_10050, not '{f}'");
    }
    Ok(Some(f))
}

/// Local `due_at`. A date-time custom field (`2026-10-02T17:00:00.000+0200`)
/// keeps its time, converted to local. A date alone (the built-in `duedate`, or
/// a custom *date* field) is pinned to 23:59:59 that day (`DAY_ONLY_DUE`): sorts after that day's
/// timed work, and views only ever show the day for a due date.
fn due_at(f: &Value, due_field: Option<&str>) -> Option<String> {
    let as_due = |v: &Value| {
        let s = v.as_str()?;
        if let Some(t) = to_local(s) {
            return Some(t);
        }
        chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").ok().map(|d| format!("{d}T{}", crate::queries::DAY_ONLY_DUE))
    };
    due_field.and_then(|k| f.get(k)).and_then(as_due).or_else(|| f.get("duedate").and_then(as_due))
}

fn map_issue(api: &Api, issue: &Value, me: &str, cursor: Option<DateTime<Utc>>, due_field: Option<&str>) -> Option<ItemUpsert> {
    let key = issue.get("key")?.as_str()?.to_string();
    let f = issue.get("fields")?;
    let s = |p: &str| f.pointer(p).and_then(Value::as_str);
    let assignee_id = s("/assignee/accountId");
    let mine = assignee_id.is_some() && assignee_id == Some(me);
    let who = if mine { "you".to_string() } else { s("/assignee/displayName").unwrap_or("Unassigned").to_string() };
    let status_name = s("/status/name").unwrap_or("Unknown");
    let done = s("/status/statusCategory/key") == Some("done");
    let comments: Vec<&Value> = f.pointer("/comment/comments").and_then(Value::as_array).map(|a| a.iter().collect()).unwrap_or_default();
    let messages = comments
        .iter()
        .rev()
        .take(KEEP_COMMENTS)
        .rev()
        .filter_map(|c| {
            Some(Message {
                at: to_local(c.get("created")?.as_str()?)?,
                author: c.pointer("/author/displayName").and_then(Value::as_str).unwrap_or("?").to_string(),
                text: adf_text(c.get("body")?),
                author_id: c.pointer("/author/accountId").and_then(Value::as_str).map(str::to_owned),
                ..Default::default()
            })
        })
        .collect();
    let mut raw = issue.clone();
    if let Some(o) = raw.as_object_mut() {
        o.remove("changelog");
    }
    Some(ItemUpsert {
        title: format!("{key} · {}", s("/summary").unwrap_or("")),
        body: f.get("description").map(adf_text).unwrap_or_default(),
        summary: Some(format!("{status_name} · {who}")),
        url: Some(api.browse(&key)),
        author: f.get("reporter").and_then(|r| {
            Some(Handle {
                handle: r.get("accountId")?.as_str()?.to_string(),
                display_name: r.get("displayName").and_then(Value::as_str).unwrap_or("?").to_string(),
            })
        }),
        occurred_at: s("/updated").and_then(to_local).or_else(|| s("/created").and_then(to_local))?,
        due_at: due_at(f, due_field),
        status: if done { "done" } else { "open" }.into(),
        msg_count: f.pointer("/comment/total").and_then(Value::as_i64),
        owner: if mine { Some("you".into()) } else { s("/assignee/displayName").map(str::to_owned) },
        bump: cursor.is_some_and(|c| bump(issue, me, c)),
        external_id: key,
        kind: "issue".into(),
        raw_json: raw,
        messages,
        ended_at: None,
        drop_event: false,
        attach_to: None,
        event: None,
    })
}

/// New signal on an issue we already had: someone else changed its status or
/// assignee, or @mentioned you in a new comment, after the cursor. Our own
/// changes (e.g. a transition sent from the outbox) never resurface an item.
fn bump(issue: &Value, me: &str, cursor: DateTime<Utc>) -> bool {
    let after = |v: &Value| v.get("created").and_then(Value::as_str).and_then(parse_time).is_some_and(|t| t > cursor);
    let by_other = |v: &Value| v.pointer("/author/accountId").and_then(Value::as_str) != Some(me);
    let changed = issue.pointer("/changelog/histories").and_then(Value::as_array).into_iter().flatten().any(|h| {
        after(h)
            && by_other(h)
            && h.get("items").and_then(Value::as_array).into_iter().flatten().any(|i| {
                let field = i.get("fieldId").or_else(|| i.get("field")).and_then(Value::as_str).unwrap_or("");
                field == "status" || field == "assignee"
            })
    });
    let mentioned = issue
        .pointer("/fields/comment/comments")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .any(|c| after(c) && by_other(c) && c.get("body").is_some_and(|b| mentions(b, me)));
    changed || mentioned
}

fn mentions(adf: &Value, account: &str) -> bool {
    if adf.get("type").and_then(Value::as_str) == Some("mention") && adf.pointer("/attrs/id").and_then(Value::as_str) == Some(account) {
        return true;
    }
    adf.get("content").and_then(Value::as_array).is_some_and(|c| c.iter().any(|n| mentions(n, account)))
}

fn transition(api: &Api, payload: &Value) -> anyhow::Result<Value> {
    let key = issue_key(payload)?;
    let want_id = payload.get("transition_id").or_else(|| payload.get("id")).and_then(|v| match v {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    });
    let want_name = payload.get("transition_name").and_then(Value::as_str).map(str::to_lowercase);
    if want_id.is_none() && want_name.is_none() {
        anyhow::bail!("jira.transition needs transition_name or transition_id");
    }
    let list = api.get(&format!("/rest/api/3/issue/{key}/transitions"))?;
    let all = list.get("transitions").and_then(Value::as_array).cloned().unwrap_or_default();
    let name_of = |t: &Value, p: &str| t.pointer(p).and_then(Value::as_str).map(str::to_lowercase);
    // Match the transition's id, its name ("Start review"), or its target status ("In Review").
    let found = all.iter().find(|t| {
        let id = t.get("id").and_then(Value::as_str);
        (want_id.is_some() && id == want_id.as_deref())
            || (want_name.is_some() && (name_of(t, "/name") == want_name || name_of(t, "/to/name") == want_name))
    });
    let Some(t) = found else {
        let names: Vec<&str> = all.iter().filter_map(|t| t.get("name").and_then(Value::as_str)).collect();
        anyhow::bail!("{key} has no transition '{}' (available: {})", want_name.or(want_id).unwrap_or_default(), names.join(", "));
    };
    let id = t.get("id").and_then(Value::as_str).unwrap_or_default();
    api.call("POST", &format!("/rest/api/3/issue/{key}/transitions"), Some(&json!({ "transition": { "id": id } })))?;
    Ok(json!({
        "key": key,
        "transition": t.get("name"),
        "to": t.pointer("/to/name"),
        "url": api.browse(&key),
    }))
}

/// Notes enqueues `{summary, assignee (a name from "@Name", or null), notes_line_id, item_id}`; Todos `{summary, assign_me, due, topic_id}`.
/// The summary is the user's own line, sent verbatim.
fn create_issue(api: &Api, payload: &Value) -> anyhow::Result<Value> {
    let summary = text(payload, "summary")?;
    let project = api.ctx.config_str("create_project")?;
    if let Some(found) = find_created(api, &project, &summary)? {
        let key = found.get("key").and_then(Value::as_str).unwrap_or_default();
        return Ok(json!({
            "key": key,
            "id": found.get("id"),
            "url": api.browse(key),
            "assignee": found.pointer("/fields/assignee/accountId"),
            "assignee_unresolved": null,
            "notes_line_id": payload.get("notes_line_id"),
            "existing": true,
        }));
    }
    let issue_type = api.ctx.source.config.get("create_issue_type").and_then(Value::as_str).unwrap_or("Task");
    let mut fields = json!({
        "project": { "key": project },
        "issuetype": { "name": issue_type },
        "summary": summary,
    });
    // A delegate name only becomes an assignee when it matches exactly one
    // assignable user; otherwise the issue is created unassigned and the receipt says so.
    let mut assignee = payload.get("account_id").and_then(Value::as_str).map(str::to_owned);
    if assignee.is_none() && payload.get("assign_me").and_then(Value::as_bool) == Some(true) {
        assignee = api.get("/rest/api/3/myself")?.get("accountId").and_then(Value::as_str).map(str::to_owned);
    }
    if let Some(due) = payload.get("due").and_then(Value::as_str) {
        fields["duedate"] = json!(due);
    }
    let mut unresolved = None;
    if assignee.is_none() {
        if let Some(name) = payload.get("assignee").and_then(Value::as_str).filter(|n| !n.trim().is_empty()) {
            let users = api.get(&format!(
                "/rest/api/3/user/assignable/search?project={}&query={}",
                urlencode(&project),
                urlencode(name)
            ))?;
            match users.as_array().map(Vec::as_slice) {
                Some([one]) => assignee = one.get("accountId").and_then(Value::as_str).map(str::to_owned),
                _ => unresolved = Some(name.to_string()),
            }
        }
    }
    if let Some(a) = &assignee {
        fields["assignee"] = json!({ "accountId": a });
    }
    let created = api.call("POST", "/rest/api/3/issue", Some(&json!({ "fields": fields })))?.json()?;
    let key = created.get("key").and_then(Value::as_str).ok_or_else(|| anyhow::anyhow!("Jira created an issue but returned no key"))?;
    Ok(json!({
        "key": key,
        "id": created.get("id"),
        "url": api.browse(key),
        "assignee": assignee,
        "assignee_unresolved": unresolved,
        "notes_line_id": payload.get("notes_line_id"),
        "existing": false,
    }))
}

/// An issue with exactly this summary that you reported in `project` within
/// the last `config.create_dedupe_hours` (24): the result of an earlier attempt
/// of this same write whose receipt got lost (timeout after Jira created it,
/// crash, ⌘R on a row that had in fact gone through).
///
/// The outbox row's own `created_at` isn't passed to `send`, so the window is
/// a fixed lower bound instead, written as a relative JQL date (`-24h`) so the
/// Jira profile's time zone doesn't matter. The trade-off: filing the same
/// summary twice on purpose within the window returns the first issue
/// (receipt `existing: true`) instead of creating a second one; a retry after
/// the window can duplicate.
///
/// `summary ~` is Jira's word search: it ignores punctuation, stems words and
/// can't match exactly, so it only narrows the search; the exact comparison
/// happens here. Its terms are the summary's words, lowercased so `AND`/`OR`/
/// `NOT` aren't read as search operators, with everything else dropped (the
/// characters Jira's text search reserves never reach it). If Jira still
/// rejects the text query (e.g. only stop words), search without it.
fn find_created(api: &Api, project: &str, summary: &str) -> anyhow::Result<Option<Value>> {
    let hours = api.ctx.source.config.get("create_dedupe_hours").and_then(Value::as_i64).unwrap_or(DEFAULT_DEDUPE_HOURS);
    if hours <= 0 {
        return Ok(None);
    }
    let base = format!("project = {} AND reporter = currentUser() AND created >= -{hours}h", jql_string(project));
    let words = search_words(summary);
    let search = |jql: &str| {
        let path = format!("/rest/api/3/search/jql?jql={}&maxResults=50&fields=summary,assignee", urlencode(jql));
        api.get(&path)
    };
    let order = " ORDER BY created DESC";
    let page = if words.is_empty() {
        search(&format!("{base}{order}"))?
    } else {
        match search(&format!("{base} AND summary ~ {}{order}", jql_string(&words))) {
            Err(e) if e.to_string().starts_with("Jira 400") => search(&format!("{base}{order}"))?,
            r => r?,
        }
    };
    Ok(page
        .get("issues")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .find(|i| i.pointer("/fields/summary").and_then(Value::as_str).map(str::trim) == Some(summary.trim()))
        .cloned())
}

/// The summary's words (letters and digits), lowercased, for a `~` search.
fn search_words(s: &str) -> String {
    s.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).take(12).collect::<Vec<_>>().join(" ").to_lowercase()
}

/// A JQL string literal: double-quoted, with `"` and `\` backslash-escaped.
/// Line breaks become spaces (a JQL string can't span lines).
fn jql_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' | '\\' => {
                out.push('\\');
                out.push(c);
            }
            '\n' | '\r' | '\t' => out.push(' '),
            _ => out.push(c),
        }
    }
    out.push('"');
    out
}

fn issue_key(payload: &Value) -> anyhow::Result<String> {
    let key = text(payload, "key")?;
    let ok = key.split_once('-').is_some_and(|(p, n)| {
        !p.is_empty()
            && p.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
            && !n.is_empty()
            && n.chars().all(|c| c.is_ascii_digit())
    });
    if !ok {
        anyhow::bail!("'{key}' isn't a Jira issue key");
    }
    Ok(key)
}

fn text(payload: &Value, field: &str) -> anyhow::Result<String> {
    payload
        .get(field)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| anyhow::anyhow!("payload needs a non-empty '{field}'"))
}

/// Jira returns `2026-09-27T10:15:30.123+0000` (no colon in the offset), which
/// isn't RFC 3339. Accept both.
fn parse_time(s: &str) -> Option<DateTime<FixedOffset>> {
    DateTime::parse_from_rfc3339(s).ok().or_else(|| DateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S%.f%z").ok())
}

fn to_local(s: &str) -> Option<String> {
    parse_time(s).and_then(|t| local_ts(&t.to_rfc3339()))
}

fn now_utc(ctx: &Ctx) -> DateTime<Utc> {
    ctx.now.and_local_timezone(chrono::Local).earliest().map(|t| t.with_timezone(&Utc)).unwrap_or_else(Utc::now)
}

/// Plain text → Atlassian Document Format: blank lines split paragraphs,
/// single newlines become hard breaks. Text is passed through unchanged.
pub fn to_adf(text: &str) -> Value {
    let paragraphs: Vec<Value> = text
        .replace("\r\n", "\n")
        .split("\n\n")
        .filter(|p| !p.trim().is_empty())
        .map(|p| {
            let mut content = vec![];
            for (i, line) in p.split('\n').enumerate() {
                if i > 0 {
                    content.push(json!({ "type": "hardBreak" }));
                }
                if !line.is_empty() {
                    content.push(json!({ "type": "text", "text": line }));
                }
            }
            json!({ "type": "paragraph", "content": content })
        })
        .collect();
    json!({ "type": "doc", "version": 1, "content": paragraphs })
}

/// ADF → plain text for search and the expanded view. Mentions keep their "@Name".
pub fn adf_text(adf: &Value) -> String {
    fn walk(n: &Value, out: &mut String) {
        match n.get("type").and_then(Value::as_str) {
            Some("text") => out.push_str(n.get("text").and_then(Value::as_str).unwrap_or("")),
            Some("mention") | Some("emoji") => out.push_str(n.pointer("/attrs/text").and_then(Value::as_str).unwrap_or("")),
            Some("hardBreak") => out.push('\n'),
            Some("media") | Some("mediaInline") => {
                let name = n.pointer("/attrs/alt").and_then(Value::as_str).unwrap_or("");
                out.push_str(&format!("[attachment] {name}"));
            }
            _ => {}
        }
        for c in n.get("content").and_then(Value::as_array).into_iter().flatten() {
            walk(c, out);
        }
        if matches!(n.get("type").and_then(Value::as_str), Some("paragraph" | "heading" | "codeBlock" | "blockquote" | "listItem" | "mediaSingle" | "mediaGroup")) {
            out.push('\n');
        }
    }
    let mut out = String::new();
    match adf {
        Value::String(s) => out.push_str(s),
        _ => walk(adf, &mut out),
    }
    out.trim().to_string()
}

/// Standard base64 for the basic-auth header (one call site; not worth a crate).
fn base64(bytes: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = (b[0] as u32) << 16 | (b[1] as u32) << 8 | b[2] as u32;
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(T[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync::http::FakeHttp;
    use crate::sync::Source;

    const MYSELF: &str = include_str!("../../../tests/fixtures/jira/myself.json");
    const PAGE1: &str = include_str!("../../../tests/fixtures/jira/search_page1.json");
    const PAGE2: &str = include_str!("../../../tests/fixtures/jira/search_page2.json");
    const TRANSITIONS: &str = include_str!("../../../tests/fixtures/jira/transitions.json");

    fn source(cursor: Option<&str>) -> Source {
        Source {
            id: 1,
            kind: "jira".into(),
            tag: "work".into(),
            name: "Work Jira".into(),
            config: json!({
                "base_url": "https://acme.atlassian.net/",
                "email": "me@acme.test",
                "create_project": "PLAT",
            }),
            cursor: cursor.map(str::to_owned),
        }
    }

    fn with_ctx<T>(src: &Source, http: &FakeHttp, f: impl FnOnce(&Ctx) -> T) -> T {
        let secret = |k: &str| (k == DEFAULT_SECRET).then(|| "tok-123".to_string());
        let now = chrono::NaiveDate::from_ymd_opt(2026, 9, 27).unwrap().and_hms_opt(10, 0, 0).unwrap();
        let ctx = Ctx { source: src, http, secret: &secret, now };
        f(&ctx)
    }

    fn polled(cursor: Option<&str>) -> (Batch, Vec<(String, String, Option<String>)>) {
        polled_with(source(cursor))
    }

    fn polled_with(src: Source) -> (Batch, Vec<(String, String, Option<String>)>) {
        let http = FakeHttp::default();
        http.on("GET", "/rest/api/3/myself", 200, MYSELF);
        http.on("GET", "/rest/api/3/search/jql", 200, PAGE1);
        http.on("GET", "nextPageToken=tok-page-2", 200, PAGE2);
        let b = with_ctx(&src, &http, |ctx| Jira.poll(ctx)).unwrap();
        (b, http.requests())
    }

    #[test]
    fn due_is_the_day_unless_a_custom_datetime_field_is_set() {
        // Built-in duedate: date only, pinned to the end of that day.
        let (b, reqs) = polled(None);
        assert_eq!(b.items[0].due_at.as_deref(), Some("2026-10-02T23:59:59"));
        assert!(!reqs[1].1.contains("customfield"));

        let mut src = source(None);
        src.config["due_field"] = json!("customfield_10050");
        let (b, reqs) = polled_with(src);
        assert!(reqs[1].1.contains("&fields=summary,status,assignee,reporter,duedate,updated,created,comment,description,project,issuetype,customfield_10050&"));
        // The custom field's real time, converted to local; it wins over duedate.
        assert_eq!(b.items[0].due_at, to_local("2026-10-01T17:00:00.000+0200"));
        assert_eq!(b.items[0].due_at, local_ts("2026-10-01T15:00:00Z"));
        // Field null and no duedate: no due.
        assert_eq!(b.items[1].due_at, None);

        // A custom field holding a plain date is still just a day.
        let f = json!({ "customfield_1": "2026-10-05", "duedate": "2026-10-09" });
        assert_eq!(due_at(&f, Some("customfield_1")).as_deref(), Some("2026-10-05T23:59:59"));
        // Missing or unparseable custom value falls back to duedate.
        assert_eq!(due_at(&f, Some("customfield_2")).as_deref(), Some("2026-10-09T23:59:59"));
        assert_eq!(due_at(&json!({ "customfield_1": "soon", "duedate": null }), Some("customfield_1")), None);

        let mut src = source(None);
        src.config["due_field"] = json!("customfield_1&jql=x");
        let http = FakeHttp::default();
        http.on("GET", "/rest/api/3/myself", 200, MYSELF);
        let err = with_ctx(&src, &http, |ctx| Jira.poll(ctx)).unwrap_err().to_string();
        assert!(err.contains("config.due_field"), "{err}");
    }

    #[test]
    fn polls_all_pages_and_maps_issues() {
        let (b, reqs) = polled(None);
        let searches: Vec<&String> = reqs.iter().filter(|r| r.1.contains("/search/jql")).map(|r| &r.1).collect();
        assert_eq!(searches.len(), 2);
        assert!(searches[0].starts_with("https://acme.atlassian.net/rest/api/3/search/jql?jql="));
        assert!(searches[0].contains("expand=changelog"));
        assert!(searches[1].contains("nextPageToken=tok-page-2"));
        // JQL is URL-encoded and bounded by updated >=.
        assert!(searches[0].contains("updated%20%3E%3D%20%22"));
        assert_eq!(b.items.len(), 3);

        let it = &b.items[0];
        assert_eq!(it.external_id, "PLAT-1170");
        assert_eq!(it.kind, "issue");
        assert_eq!(it.title, "PLAT-1170 · Retry budget for ingest workers");
        assert_eq!(it.summary.as_deref(), Some("In Review · you"));
        assert_eq!(it.owner.as_deref(), Some("you"));
        assert_eq!(it.url.as_deref(), Some("https://acme.atlassian.net/browse/PLAT-1170"));
        assert_eq!(it.author.as_ref().unwrap().handle, "acc-jordan");
        assert_eq!(it.due_at.as_deref(), Some("2026-10-02T23:59:59"));
        assert_eq!(it.status, "open");
        assert_eq!(it.msg_count, Some(2));
        assert_eq!(it.occurred_at, to_local("2026-09-26T14:05:00.000+0000").unwrap());
        assert_eq!(it.body, "Workers retry forever on 5xx.\nCap it.");
        assert_eq!(it.messages.len(), 2);
        assert_eq!(it.messages[1].text, "@Sam can you look?");
        assert!(it.raw_json.get("changelog").is_none());
        assert!(!it.bump, "first sync never bumps; new items are new signal anyway");

        let other = &b.items[1];
        assert_eq!(other.owner.as_deref(), Some("Kim Lee"));
        assert_eq!(other.summary.as_deref(), Some("Done · Kim Lee"));
        assert_eq!(other.status, "done");
        let unassigned = &b.items[2];
        assert_eq!(unassigned.owner, None);
        assert_eq!(unassigned.summary.as_deref(), Some("To Do · Unassigned"));

        // Cursor = newest `updated`, exact to the millisecond, in UTC.
        assert_eq!(b.cursor.as_deref(), Some("2026-09-27T08:30:12.345Z"));
    }

    #[test]
    fn cursor_bounds_the_query_and_never_goes_backwards() {
        let (b, reqs) = polled(Some("2026-09-28T00:00:00.000Z"));
        // Bound is the cursor minus a day (time zone slack), minute precision.
        assert!(reqs[1].1.contains(&urlencode("updated >= \"2026-09-27 00:00\"")));
        assert_eq!(b.cursor.as_deref(), Some("2026-09-28T00:00:00.000Z"));
    }

    #[test]
    fn bumps_on_others_status_change_or_new_mention_only() {
        // Cursor between the fixtures' older and newer events.
        let (b, _) = polled(Some("2026-09-26T12:00:00.000Z"));
        let by_key = |k: &str| b.items.iter().find(|i| i.external_id == k).unwrap();
        // PLAT-1170: new comment by Jordan mentioning me (acc-me) after the cursor.
        assert!(by_key("PLAT-1170").bump);
        // PLAT-1150: status changed to Done by Kim after the cursor.
        assert!(by_key("PLAT-1150").bump);
        // PLAT-1201: only my own assignee change after the cursor, and a mention of someone else.
        assert!(!by_key("PLAT-1201").bump);

        // Everything is older than a later cursor: nothing bumps.
        let (b, _) = polled(Some("2026-09-27T09:00:00.000Z"));
        assert!(b.items.iter().all(|i| !i.bump));
    }

    #[test]
    fn missing_secret_or_config_is_a_clear_error() {
        let http = FakeHttp::default();
        let mut src = source(None);
        src.config["secret"] = json!("other_name");
        let err = with_ctx(&src, &http, |ctx| Jira.poll(ctx)).unwrap_err().to_string();
        assert!(err.contains("missing secret 'other_name'"), "{err}");
        let src = Source { config: json!({ "email": "x" }), ..source(None) };
        let err = with_ctx(&src, &http, |ctx| Jira.poll(ctx)).unwrap_err().to_string();
        assert!(err.contains("config.base_url"), "{err}");
    }

    #[test]
    fn surfaces_jira_error_messages_without_the_token() {
        let http = FakeHttp::default();
        http.on("GET", "/myself", 200, MYSELF);
        http.on(
            "GET",
            "/search/jql",
            400,
            r#"{"errorMessages":["Field 'watcher' does not exist or you do not have permission to view it."],"errors":{"jql":"bad"}}"#,
        );
        let src = source(None);
        let err = with_ctx(&src, &http, |ctx| Jira.poll(ctx)).unwrap_err().to_string();
        assert_eq!(err, "Jira 400: Field 'watcher' does not exist or you do not have permission to view it.; jql: bad");
        http.on("GET", "/myself", 401, "");
        let err = with_ctx(&src, &http, |ctx| Jira.poll(ctx)).unwrap_err().to_string();
        assert_eq!(err, "Jira 401: email or API token rejected");
        assert!(!err.contains("tok-123"));
    }

    fn send(op: &str, payload: Value, http: &FakeHttp) -> anyhow::Result<Value> {
        let src = source(None);
        with_ctx(&src, http, |ctx| Jira.send(ctx, op, &payload))
    }

    #[test]
    fn transition_looks_up_by_name_or_target_status() {
        let http = FakeHttp::default();
        http.on("GET", "/rest/api/3/issue/PLAT-1170/transitions", 200, TRANSITIONS);
        http.on("POST", "/rest/api/3/issue/PLAT-1170/transitions", 204, "");
        let r = send("jira.transition", json!({ "key": "PLAT-1170", "transition_name": "done" }), &http).unwrap();
        assert_eq!(r["key"], "PLAT-1170");
        assert_eq!(r["to"], "Done");
        assert_eq!(r["url"], "https://acme.atlassian.net/browse/PLAT-1170");
        let reqs = http.requests();
        let post = reqs.last().unwrap();
        assert_eq!(post.0, "POST");
        assert_eq!(post.1, "https://acme.atlassian.net/rest/api/3/issue/PLAT-1170/transitions");
        assert_eq!(serde_json::from_str::<Value>(post.2.as_deref().unwrap()).unwrap(), json!({ "transition": { "id": "31" } }));

        // By transition name ("Start review" goes to "In Review") and by id.
        send("jira.transition", json!({ "key": "PLAT-1170", "transition_name": "Start review" }), &http).unwrap();
        assert!(http.requests().last().unwrap().2.as_deref().unwrap().contains("\"21\""));
        send("jira.transition", json!({ "key": "PLAT-1170", "transition_id": 11 }), &http).unwrap();
        assert!(http.requests().last().unwrap().2.as_deref().unwrap().contains("\"11\""));

        let err = send("jira.transition", json!({ "key": "PLAT-1170", "transition_name": "Nope" }), &http).unwrap_err();
        assert!(err.to_string().contains("available: To Do, Start review, Done"), "{err}");
    }

    #[test]
    fn comment_sends_adf_and_returns_link() {
        let http = FakeHttp::default();
        http.on("POST", "/rest/api/3/issue/PLAT-1170/comment", 201, r#"{"id":"10042","body":{}}"#);
        let r = send("jira.comment", json!({ "key": "PLAT-1170", "body": "Looks good.\nShip it.\n\nThanks" }), &http).unwrap();
        assert_eq!(r["url"], "https://acme.atlassian.net/browse/PLAT-1170?focusedCommentId=10042");
        let reqs = http.requests();
        let body: Value = serde_json::from_str(reqs[0].2.as_deref().unwrap()).unwrap();
        assert_eq!(
            body,
            json!({ "body": { "type": "doc", "version": 1, "content": [
                { "type": "paragraph", "content": [
                    { "type": "text", "text": "Looks good." }, { "type": "hardBreak" }, { "type": "text", "text": "Ship it." } ] },
                { "type": "paragraph", "content": [ { "type": "text", "text": "Thanks" } ] }
            ] } })
        );
        assert!(send("jira.comment", json!({ "key": "PLAT-1170", "body": "  " }), &http).is_err());
        assert!(send("jira.comment", json!({ "key": "../x", "body": "hi" }), &http).is_err());
    }

    #[test]
    fn assign_puts_account_id() {
        let http = FakeHttp::default();
        http.on("PUT", "/rest/api/3/issue/PLAT-1201/assignee", 204, "");
        send("jira.assign", json!({ "key": "PLAT-1201", "account_id": "acc-kim" }), &http).unwrap();
        let reqs = http.requests();
        assert_eq!(reqs[0].0, "PUT");
        assert_eq!(reqs[0].1, "https://acme.atlassian.net/rest/api/3/issue/PLAT-1201/assignee");
        assert_eq!(serde_json::from_str::<Value>(reqs[0].2.as_deref().unwrap()).unwrap(), json!({ "accountId": "acc-kim" }));
    }

    const NO_ISSUES: &str = r#"{"issues":[],"isLast":true}"#;
    const CREATED: &str = include_str!("../../../tests/fixtures/jira/search_created.json");

    #[test]
    fn create_issue_from_notes_payload_resolves_a_unique_delegate() {
        let http = FakeHttp::default();
        http.on("GET", "/rest/api/3/search/jql", 200, NO_ISSUES);
        http.on("GET", "/rest/api/3/user/assignable/search", 200, r#"[{"accountId":"acc-kim","displayName":"Kim Lee","active":true}]"#);
        http.on("POST", "/rest/api/3/issue", 201, r#"{"id":"10500","key":"PLAT-1300","self":"https://acme.atlassian.net/rest/api/3/issue/10500"}"#);
        // Exactly what queries/notes.rs enqueues.
        let payload = json!({ "summary": "Draft retry budget RFC @Kim", "assignee": "Kim", "notes_line_id": 7, "item_id": 42 });
        let r = send("jira.create_issue", payload, &http).unwrap();
        assert_eq!(r["key"], "PLAT-1300");
        assert_eq!(r["url"], "https://acme.atlassian.net/browse/PLAT-1300");
        assert_eq!(r["assignee"], "acc-kim");
        assert_eq!(r["existing"], false);
        let reqs = http.requests();
        assert!(reqs[0].1.contains("/rest/api/3/search/jql?jql="), "looks for an earlier attempt first");
        assert!(reqs[1].1.ends_with("/rest/api/3/user/assignable/search?project=PLAT&query=Kim"));
        assert_eq!(reqs[2].1, "https://acme.atlassian.net/rest/api/3/issue");
        assert_eq!(
            serde_json::from_str::<Value>(reqs[2].2.as_deref().unwrap()).unwrap(),
            json!({ "fields": {
                "project": { "key": "PLAT" }, "issuetype": { "name": "Task" },
                "summary": "Draft retry budget RFC @Kim", "assignee": { "accountId": "acc-kim" } } })
        );

        // Ambiguous delegate: created unassigned, and the receipt says why.
        http.on("GET", "/rest/api/3/user/assignable/search", 200, r#"[{"accountId":"a"},{"accountId":"b"}]"#);
        let r = send("jira.create_issue", json!({ "summary": "x", "assignee": "Sam" }), &http).unwrap();
        assert_eq!(r["assignee_unresolved"], "Sam");
        assert!(http.requests().last().unwrap().2.as_deref().unwrap().find("assignee").is_none());

        // No delegate: no user lookup (the dedupe search, then the create).
        let before = http.requests().len();
        send("jira.create_issue", json!({ "summary": "y", "assignee": null }), &http).unwrap();
        assert_eq!(http.requests().len(), before + 2);
    }

    #[test]
    fn create_issue_retry_returns_the_issue_an_earlier_attempt_created() {
        let http = FakeHttp::default();
        http.on("GET", "/rest/api/3/search/jql", 200, CREATED);
        let payload = json!({ "summary": "Draft retry budget RFC @Kim", "assignee": "Kim", "notes_line_id": 7, "item_id": 42 });
        let r = send("jira.create_issue", payload, &http).unwrap();
        // Exact summary match (not the similar "(v2)" one), and nothing created.
        assert_eq!(r["key"], "PLAT-1300");
        assert_eq!(r["id"], "10500");
        assert_eq!(r["url"], "https://acme.atlassian.net/browse/PLAT-1300");
        assert_eq!(r["assignee"], "acc-kim");
        assert_eq!(r["existing"], true);
        assert_eq!(r["notes_line_id"], 7);
        let reqs = http.requests();
        assert_eq!(reqs.len(), 1);
        assert!(reqs.iter().all(|r| r.0 == "GET"));
        let want = "project = \"PLAT\" AND reporter = currentUser() AND created >= -24h AND summary ~ \"draft retry budget rfc kim\" ORDER BY created DESC";
        assert!(reqs[0].1.contains(&format!("jql={}&", urlencode(want))), "{}", reqs[0].1);
        assert!(reqs[0].1.ends_with("&maxResults=50&fields=summary,assignee"));

        // Only a similar summary: create.
        http.on("POST", "/rest/api/3/issue", 201, r#"{"id":"10503","key":"PLAT-1303"}"#);
        let r = send("jira.create_issue", json!({ "summary": "Draft retry budget RFC" }), &http).unwrap();
        assert_eq!((r["key"].as_str(), r["existing"].as_bool()), (Some("PLAT-1303"), Some(false)));
    }

    #[test]
    fn create_issue_dedupe_survives_odd_summaries() {
        let http = FakeHttp::default();
        // Jira rejects the text query: search again without it, still compare exactly.
        http.on("GET", "summary%20~", 400, r#"{"errorMessages":["The text query 'the' for field 'summary' is not allowed."],"errors":{}}"#);
        http.on("GET", "created%20%3E%3D%20-24h%20ORDER%20BY", 200, r#"{"issues":[{"id":"1","key":"PLAT-9","fields":{"summary":"To be or not"}}],"isLast":true}"#);
        let r = send("jira.create_issue", json!({ "summary": "To be or not" }), &http).unwrap();
        assert_eq!((r["key"].as_str(), r["existing"].as_bool()), (Some("PLAT-9"), Some(true)));
        assert_eq!(http.requests().len(), 2);

        // Only punctuation: no text clause at all. Other search errors fail the write.
        let http = FakeHttp::default();
        http.on("GET", "/search/jql", 500, "");
        let err = send("jira.create_issue", json!({ "summary": "?!" }), &http).unwrap_err().to_string();
        assert!(err.starts_with("Jira 500"), "{err}");
        assert!(!http.requests()[0].1.contains("summary%20~"));
        assert!(http.requests().iter().all(|r| r.0 == "GET"), "never creates blind");

        // create_dedupe_hours: 0 turns the check off.
        let http = FakeHttp::default();
        http.on("POST", "/rest/api/3/issue", 201, r#"{"id":"2","key":"PLAT-10"}"#);
        let mut src = source(None);
        src.config["create_dedupe_hours"] = json!(0);
        with_ctx(&src, &http, |ctx| Jira.send(ctx, "jira.create_issue", &json!({ "summary": "x" }))).unwrap();
        assert_eq!(http.requests().len(), 1);
    }

    #[test]
    fn jql_strings_are_escaped() {
        assert_eq!(jql_string("PLAT"), "\"PLAT\"");
        assert_eq!(jql_string(r#"say "hi" \o/"#), r#""say \"hi\" \\o/""#);
        assert_eq!(jql_string("a\nb"), "\"a b\"");
        assert_eq!(search_words("Fix \"retry\" AND [ingest]: 5xx -> OR cap?"), "fix retry and ingest 5xx or cap");
        assert_eq!(search_words("Überprüfen: Café"), "überprüfen café");
        assert_eq!(search_words("?!"), "");
    }

    #[test]
    fn create_issue_error_shows_field_errors() {
        let http = FakeHttp::default();
        http.on("GET", "/rest/api/3/search/jql", 200, NO_ISSUES);
        http.on("POST", "/rest/api/3/issue", 400, r#"{"errorMessages":[],"errors":{"issuetype":"Specify a valid issue type"}}"#);
        let err = send("jira.create_issue", json!({ "summary": "x" }), &http).unwrap_err().to_string();
        assert_eq!(err, "Jira 400: issuetype: Specify a valid issue type");
    }

    #[test]
    fn adf_media_shows_as_an_attachment() {
        let doc = json!({ "type": "doc", "content": [
            { "type": "paragraph", "content": [{ "type": "text", "text": "See:" }] },
            { "type": "mediaSingle", "content": [{ "type": "media", "attrs": { "type": "file", "id": "x", "alt": "trace.png" } }] },
        ] });
        assert_eq!(adf_text(&doc), "See:\n[attachment] trace.png");
    }

    #[test]
    fn base64_matches_rfc4648() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"me@acme.test:tok"), "bWVAYWNtZS50ZXN0OnRvaw==");
    }
}
