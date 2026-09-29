//! Gmail (the work inbox), one item per Inbox thread; see docs/sources/gmail.md.

use super::gcal::oauth::{self, TokenCache};
use super::gcal::{enc, name_from_email, strip_html, Api};
use crate::sync::{local_from_unix, Batch, Connector, Ctx, Handle, ItemUpsert, Message};
use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashSet};

pub const API: &str = "https://gmail.googleapis.com/gmail/v1/users/me";
const QUERY: &str = "in:inbox";
/// `threads.get` calls per poll; the rest of the queue waits for the next one.
const FETCH_PER_POLL: usize = 200;
/// Pages of 500 for `threads.list` and `history.list`.
const MAX_PAGES: usize = 100;
/// Characters kept per message body.
const MAX_CHARS: usize = 2000;

pub struct Gmail;

impl Connector for Gmail {
    fn poll(&self, ctx: &Ctx) -> anyhow::Result<Batch> {
        poll_with(ctx, &oauth::CACHE)
    }

    fn send(&self, ctx: &Ctx, op: &str, payload: &Value) -> anyhow::Result<Value> {
        let api = Api { ctx, cache: &oauth::CACHE };
        match op {
            "gmail.file" | "gmail.mark_read" => file(&api, payload),
            "gmail.archive" => modify(&api, payload, json!({ "removeLabelIds": ["INBOX"] })),
            "gmail.trash" => thread_post(&api, payload, "trash"),
            "gmail.restore" => {
                thread_post(&api, payload, "untrash")?;
                modify(&api, payload, json!({ "addLabelIds": ["INBOX"] }))
            }
            _ => anyhow::bail!("Gmail doesn't support '{op}'"),
        }
    }
}

#[derive(Default, Serialize, Deserialize)]
struct Cursor {
    history_id: Option<String>,
    /// Threads still to read with `threads.get`, in order.
    #[serde(default)]
    queue: Vec<String>,
    /// Inbox thread id -> newest message (`internalDate`, ms).
    #[serde(default)]
    tracked: BTreeMap<String, i64>,
}

fn poll_with(ctx: &Ctx, cache: &TokenCache) -> anyhow::Result<Batch> {
    let api = Api { ctx, cache };
    let mut cur: Cursor = ctx.source.cursor.as_deref().and_then(|s| serde_json::from_str(s).ok()).unwrap_or_default();
    let mut batch = Batch::default();
    let changed = match &cur.history_id {
        Some(h) => history(&api, h)?,
        None => None,
    };
    match changed {
        Some((threads, id)) => {
            for (t, may_be_in_inbox) in threads {
                if may_be_in_inbox || cur.tracked.contains_key(&t) {
                    cur.queue.push(t);
                }
            }
            cur.history_id = Some(id);
        }
        // First sync, or the history id is too old: list the whole Inbox.
        None => {
            let profile = api.get(&format!("{API}/profile"))?.ok()?.json()?;
            let id = id_str(&profile["historyId"]).ok_or_else(|| anyhow::anyhow!("Gmail profile had no historyId"))?;
            let inbox = list_inbox(&api)?;
            let listed: HashSet<&String> = inbox.iter().collect();
            for t in cur.tracked.keys().filter(|t| !listed.contains(t)) {
                batch.seen.push(t.clone());
            }
            cur.tracked.retain(|t, _| listed.contains(t));
            cur.queue.splice(0..0, inbox.iter().cloned());
            cur.history_id = Some(id);
        }
    }
    let mut dedup = HashSet::new();
    cur.queue.retain(|t| dedup.insert(t.clone()));

    let n = cur.queue.len().min(FETCH_PER_POLL);
    let now: Vec<String> = cur.queue.drain(..n).collect();
    let account = ctx.source.config.get("account").and_then(Value::as_str).unwrap_or("0");
    for (i, t) in now.iter().enumerate() {
        let resp = api.get(&format!("{API}/threads/{}?format=full", enc(t)))?;
        if rate_limited(&resp) {
            cur.queue.splice(0..0, now[i..].iter().cloned());
            break;
        }
        let thread = if resp.status == 404 { None } else { Some(resp.ok()?.json()?) };
        match thread.and_then(|th| map_thread(t, &th, account, cur.tracked.get(t).copied())) {
            Some((item, last)) => {
                cur.tracked.insert(t.clone(), last);
                batch.items.push(item);
            }
            None => {
                if cur.tracked.remove(t).is_some() {
                    batch.seen.push(t.clone());
                }
            }
        }
    }
    batch.cursor = Some(serde_json::to_string(&cur)?);
    Ok(batch)
}

fn rate_limited(resp: &crate::sync::http::Response) -> bool {
    resp.status == 429 || (resp.status == 403 && resp.body.to_ascii_lowercase().contains("ratelimitexceeded"))
}

/// Gmail sends ids as strings; tolerate numbers.
fn id_str(v: &Value) -> Option<String> {
    v.as_str().map(str::to_owned).or_else(|| v.as_u64().map(|n| n.to_string()))
}

/// Changed threads (flagged when they may now be in the Inbox) and the new history id; `None` means re-list.
fn history(api: &Api, start: &str) -> anyhow::Result<Option<(Vec<(String, bool)>, String)>> {
    let base = format!("{API}/history?startHistoryId={}&maxResults=500", enc(start));
    let mut out = Vec::new();
    let mut page: Option<String> = None;
    for _ in 0..MAX_PAGES {
        let url = match &page {
            Some(p) => format!("{base}&pageToken={}", enc(p)),
            None => base.clone(),
        };
        let resp = api.get(&url)?;
        if resp.status == 404 {
            return Ok(None);
        }
        let v = resp.ok()?.json()?;
        for h in v.get("history").and_then(Value::as_array).into_iter().flatten() {
            changes(h, &mut out);
        }
        match v.get("nextPageToken").and_then(Value::as_str) {
            Some(p) => page = Some(p.to_string()),
            None => return Ok(Some((out, id_str(&v["historyId"]).unwrap_or_else(|| start.to_string())))),
        }
    }
    Ok(None)
}

fn changes(h: &Value, out: &mut Vec<(String, bool)>) {
    let has = |labels: &Value, l: &str| labels.as_array().is_some_and(|a| a.iter().any(|x| x == l));
    for key in ["messagesAdded", "labelsAdded", "labelsRemoved", "messagesDeleted"] {
        for c in h.get(key).and_then(Value::as_array).into_iter().flatten() {
            let Some(t) = c.pointer("/message/threadId").and_then(Value::as_str) else { continue };
            let inbox_moved = ["INBOX", "TRASH", "SPAM"].iter().any(|l| has(&c["labelIds"], l));
            let may_be_in_inbox = match key {
                "messagesAdded" => has(&c["message"]["labelIds"], "INBOX"),
                "labelsAdded" if inbox_moved => has(&c["labelIds"], "INBOX"),
                "labelsAdded" | "labelsRemoved" if !inbox_moved => continue,
                _ => false,
            };
            out.push((t.to_string(), may_be_in_inbox));
        }
    }
}

/// Every thread matching `QUERY`, newest first.
fn list_inbox(api: &Api) -> anyhow::Result<Vec<String>> {
    let base = format!("{API}/threads?q={}&maxResults=500", enc(QUERY));
    let mut out = Vec::new();
    let mut page: Option<String> = None;
    for _ in 0..MAX_PAGES {
        let url = match &page {
            Some(p) => format!("{base}&pageToken={}", enc(p)),
            None => base.clone(),
        };
        let v = api.get(&url)?.ok()?.json()?;
        let ids = v.get("threads").and_then(Value::as_array).into_iter().flatten().filter_map(|t| t["id"].as_str());
        out.extend(ids.map(str::to_owned));
        match v.get("nextPageToken").and_then(Value::as_str) {
            Some(p) => page = Some(p.to_string()),
            None => return Ok(out),
        }
    }
    anyhow::bail!("more than {} threads in the Inbox; not syncing a partial list", out.len())
}

fn labeled(m: &Value, l: &str) -> bool {
    m["labelIds"].as_array().is_some_and(|a| a.iter().any(|x| x == l))
}

fn unread_in_inbox(m: &Value) -> bool {
    labeled(m, "UNREAD") && labeled(m, "INBOX")
}

fn kept(m: &Value) -> bool {
    !["TRASH", "SPAM", "DRAFT"].iter().any(|l| labeled(m, l))
}

fn internal_ms(m: &Value) -> i64 {
    m["internalDate"].as_str().and_then(|s| s.parse().ok()).or_else(|| m["internalDate"].as_i64()).unwrap_or(0)
}

fn header<'a>(m: &'a Value, name: &str) -> Option<&'a str> {
    m.pointer("/payload/headers")?
        .as_array()?
        .iter()
        .find(|h| h["name"].as_str().is_some_and(|n| n.eq_ignore_ascii_case(name)))?["value"]
        .as_str()
}

/// The item (every message, read or not) and its newest message time; `None` when the thread isn't in the Inbox.
fn map_thread(id: &str, th: &Value, account: &str, prev: Option<i64>) -> Option<(ItemUpsert, i64)> {
    let all = th["messages"].as_array()?;
    let mut msgs: Vec<&Value> = all.iter().filter(|m| kept(m)).collect();
    if !msgs.iter().any(|m| labeled(m, "INBOX")) {
        return None;
    }
    msgs.sort_by_key(|m| internal_ms(m));
    let (first, last) = (*msgs.first()?, *msgs.last()?);
    let last_ms = internal_ms(last);
    let at = |m: &Value| local_from_unix(internal_ms(m) as f64 / 1000.0).unwrap_or_default();
    let messages: Vec<Message> = msgs
        .iter()
        .map(|m| Message {
            at: at(m),
            author: sender(m).map(|h| h.display_name).unwrap_or_else(|| "unknown".into()),
            text: body_text(m),
            ..Default::default()
        })
        .collect();
    let title = header(&all[0], "Subject").map(str::trim).filter(|s| !s.is_empty()).unwrap_or("(no subject)").to_string();
    let item = ItemUpsert {
        external_id: id.to_string(),
        kind: "email".into(),
        title,
        body: messages.iter().map(|m| format!("{}: {}", m.author, m.text)).collect::<Vec<_>>().join("\n"),
        summary: messages.last().map(|m| clip(&m.text.split_whitespace().collect::<Vec<_>>().join(" "), 140)),
        url: Some(format!("https://mail.google.com/mail/u/{account}/#all/{id}")),
        author: sender(last),
        occurred_at: at(first),
        ended_at: Some(at(last)),
        status: "open".into(),
        msg_count: Some(messages.len() as i64),
        raw_json: json!({
            "thread_id": id,
            "last_ms": last_ms,
            "msgs": msgs.iter().filter_map(|m| m["id"].as_str()).collect::<Vec<_>>(),
        }),
        messages,
        bump: prev.is_none_or(|p| last_ms > p),
        ..Default::default()
    };
    Some((item, last_ms))
}

/// `Dana Kim <dana@acme.com>`, `"Lee, Sam" <sam@acme.com>` or a bare address.
fn sender(m: &Value) -> Option<Handle> {
    let from = header(m, "From")?;
    let (name, email) = match (from.rfind('<'), from.rfind('>')) {
        (Some(a), Some(b)) if a < b => (from[..a].trim().trim_matches('"').trim(), from[a + 1..b].trim()),
        _ => ("", from.trim()),
    };
    if email.is_empty() {
        return None;
    }
    let email = email.to_ascii_lowercase();
    let display_name = if name.is_empty() { name_from_email(&email) } else { name.to_string() };
    let local = email.split('@').next().unwrap_or_default();
    let relay = ["noreply", "no-reply", "notification"].iter().any(|n| local.contains(n)) || ["jira", "confluence"].contains(&local);
    let group = name.contains(" via ") && !name.contains("(via ");
    let shared = relay || group;
    let handle = if shared { format!("{email} {display_name}") } else { email };
    Some(Handle { handle, display_name })
}

/// text/plain if there is one, else the HTML's text; quoted history cut off.
fn body_text(m: &Value) -> String {
    let p = &m["payload"];
    let text = match part(p, "text/plain") {
        Some(t) => t,
        None => part(p, "text/html").map(|h| html_text(&h)).unwrap_or_else(|| entities(m["snippet"].as_str().unwrap_or(""))),
    };
    clip(&unquote(&tidy(&text)), MAX_CHARS)
}

/// First body of this MIME type, depth first, attachments skipped.
fn part(p: &Value, mime: &str) -> Option<String> {
    let is_attachment = p["filename"].as_str().is_some_and(|f| !f.is_empty());
    if !is_attachment && p["mimeType"].as_str().is_some_and(|t| t.eq_ignore_ascii_case(mime)) {
        if let Some(data) = p.pointer("/body/data").and_then(Value::as_str) {
            let engine = base64::engine::GeneralPurpose::new(
                &base64::alphabet::URL_SAFE,
                base64::engine::GeneralPurposeConfig::new().with_decode_padding_mode(base64::engine::DecodePaddingMode::Indifferent),
            );
            if let Ok(bytes) = engine.decode(data) {
                return Some(String::from_utf8_lossy(&bytes).into_owned());
            }
        }
    }
    p["parts"].as_array()?.iter().find_map(|c| part(c, mime))
}

fn html_text(h: &str) -> String {
    let hidden = regex::Regex::new(r"(?is)<head\b.*?</head>|<style\b.*?</style>|<script\b.*?</script>").expect("static regex");
    let blocks = regex::Regex::new(r"(?i)</(div|tr|li|h[1-6]|table|blockquote)>").expect("static regex");
    let s = hidden.replace_all(h, "");
    entities(&strip_html(&blocks.replace_all(&s, "\n")))
}

fn entities(s: &str) -> String {
    s.replace("&#39;", "'").replace("&#x27;", "'").replace("&rsquo;", "’").replace("&amp;", "&").replace("&quot;", "\"")
}

/// Whitespace collapsed within lines, at most one blank line in a row.
fn tidy(s: &str) -> String {
    let mut out: Vec<String> = Vec::new();
    for line in s.lines() {
        let l = line.split_whitespace().collect::<Vec<_>>().join(" ");
        if !l.is_empty() || out.last().is_some_and(|p| !p.is_empty()) {
            out.push(l);
        }
    }
    out.join("\n").trim().to_string()
}

/// Cut at the quoted reply ("On … wrote:", "> …", Outlook's original message).
fn unquote(s: &str) -> String {
    let re = regex::Regex::new(r"(?m)^>|^On [^\n]*(\n[^\n]*)?wrote:[ \t]*$|^-{2,} ?Original Message ?-{2,}|^From: [^\n]*\n(Sent|Date): ")
        .expect("static regex");
    match re.find(s) {
        Some(m) if !s[..m.start()].trim().is_empty() => s[..m.start()].trim().to_string(),
        _ => s.to_string(),
    }
}

fn clip(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        return s.to_string();
    }
    let mut out: String = s.chars().take(n - 1).collect();
    out.push('…');
    out
}

fn thread_id(payload: &Value) -> anyhow::Result<&str> {
    payload["thread_id"].as_str().filter(|t| !t.is_empty()).ok_or_else(|| anyhow::anyhow!("payload needs thread_id"))
}

/// `gmail.file {thread_id, message_ids?, label?}`: adds the label (by name) and marks the thread read, unless
/// unread mail you haven't seen here arrived since; then it only adds the label.
fn file(api: &Api, payload: &Value) -> anyhow::Result<Value> {
    let thread = thread_id(payload)?;
    let mut mark_read = true;
    if let Some(known) = payload["message_ids"].as_array() {
        let th = api.get(&format!("{API}/threads/{}?format=minimal", enc(thread)))?.ok()?.json()?;
        let msgs = th["messages"].as_array().into_iter().flatten();
        mark_read = !msgs.filter(|m| unread_in_inbox(m)).any(|m| !known.contains(&m["id"]));
    }
    let mut change = json!({});
    if mark_read {
        change["removeLabelIds"] = json!(["UNREAD"]);
    }
    if let Some(name) = payload["label"].as_str() {
        let labels = api.get(&format!("{API}/labels"))?.ok()?.json()?;
        let id = labels["labels"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|l| l["name"].as_str() == Some(name))
            .and_then(|l| l["id"].as_str())
            .ok_or_else(|| anyhow::anyhow!("no Gmail label named '{name}'; create it in Gmail, then retry"))?;
        change["addLabelIds"] = json!([id]);
    }
    let mut receipt = if change.as_object().is_some_and(|o| o.is_empty()) { json!({ "thread_id": thread }) } else { modify(api, payload, change)? };
    if !mark_read {
        receipt["left_unread"] = json!("new mail arrived since");
    }
    Ok(receipt)
}

fn modify(api: &Api, payload: &Value, change: Value) -> anyhow::Result<Value> {
    let thread = thread_id(payload)?;
    let v = api.request("POST", &format!("{API}/threads/{}/modify", enc(thread)), Some(&change), &[])?.ok()?.json()?;
    Ok(json!({ "thread_id": v["id"].as_str().unwrap_or(thread), "history_id": v["historyId"] }))
}

fn thread_post(api: &Api, payload: &Value, action: &str) -> anyhow::Result<Value> {
    let thread = thread_id(payload)?;
    let v = api.request("POST", &format!("{API}/threads/{}/{action}", enc(thread)), None, &[])?.ok()?.json()?;
    Ok(json!({ "thread_id": v["id"].as_str().unwrap_or(thread) }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync::connectors::gcal::tests::http as google;
    use crate::sync::http::FakeHttp;
    use crate::sync::Source;

    macro_rules! fixture {
        ($name:literal) => {
            include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/gmail/", $name))
        };
    }

    const A: &str = "18f0a1b2c3d4e5f6";
    const B: &str = "18f0a1b2c3d4e5f7";
    const C: &str = "18f0a1b2c3d4e5f8";

    fn source(cursor: Option<String>) -> Source {
        Source { id: 1, kind: "gmail".into(), tag: "work".into(), name: "Work mail".into(), config: json!({}), cursor }
    }

    fn secrets(k: &str) -> Option<String> {
        match k {
            "gcal.client_id" => Some("cid.apps.googleusercontent.com".into()),
            "gcal.client_secret" => Some("GOCSPX-secret".into()),
            "gmail.refresh_token" => Some("1//0gGmailRefresh".into()),
            _ => None,
        }
    }

    fn poll(http: &FakeHttp, cursor: Option<String>) -> Batch {
        let src = source(cursor);
        let secret = secrets;
        let ctx = Ctx { source: &src, http, secret: &secret, now: crate::sync::connectors::gcal::tests::now() };
        poll_with(&ctx, &TokenCache::new()).unwrap()
    }

    fn cursor(b: &Batch) -> Value {
        serde_json::from_str(b.cursor.as_deref().unwrap()).unwrap()
    }

    fn gets(http: &FakeHttp) -> Vec<String> {
        http.requests().into_iter().filter(|r| r.0 == "GET").map(|r| r.1).collect()
    }

    fn item<'a>(b: &'a Batch, id: &str) -> &'a ItemUpsert {
        b.items.iter().find(|i| i.external_id == id).unwrap_or_else(|| panic!("no item {id}"))
    }

    fn utc(h: u32, m: u32) -> String {
        let t = chrono::NaiveDate::from_ymd_opt(2026, 9, 28).unwrap().and_hms_opt(h, m, 0).unwrap().and_utc();
        local_from_unix(t.timestamp() as f64).unwrap()
    }

    fn full_list(http: &FakeHttp) {
        http.on("GET", "/users/me/profile", 200, fixture!("profile.json"));
        http.on("GET", "/threads?q=", 200, fixture!("threads_page1.json"));
        http.on("GET", "pageToken=09876543210987654321", 200, fixture!("threads_page2.json"));
        http.on("GET", &format!("/threads/{A}?"), 200, fixture!("thread_a.json"));
        http.on("GET", &format!("/threads/{B}?"), 200, fixture!("thread_b.json"));
        http.on("GET", &format!("/threads/{C}?"), 200, fixture!("thread_c.json"));
    }

    fn first_sync() -> (FakeHttp, Batch) {
        let http = google();
        full_list(&http);
        let b = poll(&http, None);
        (http, b)
    }

    #[test]
    fn first_sync_brings_every_inbox_thread_as_new() {
        let (http, b) = first_sync();
        let g = gets(&http);
        assert_eq!(g[0], format!("{API}/profile"));
        assert_eq!(g[1], format!("{API}/threads?q=in%3Ainbox&maxResults=500"));
        assert!(g[2].ends_with("&pageToken=09876543210987654321"));
        assert_eq!(g[3], format!("{API}/threads/{A}?format=full"));
        assert_eq!(g.len(), 6);
        let ids: Vec<&str> = b.items.iter().map(|i| i.external_id.as_str()).collect();
        assert_eq!(ids, [A, B, C]);
        assert!(b.items.iter().all(|i| i.bump && i.kind == "email"), "not a quiet first sync");
        assert!(b.seen.is_empty());
        let c = cursor(&b);
        assert_eq!(c["history_id"], "4201");
        assert_eq!(c["queue"], json!([]));
        assert_eq!(c["tracked"].as_object().unwrap().len(), 3);
        let body = http.requests().iter().find(|r| r.1.contains("oauth2")).unwrap().2.clone().unwrap();
        assert!(body.contains("refresh_token=1%2F%2F0gGmailRefresh"), "the source's own refresh token");
    }

    #[test]
    fn thread_becomes_one_item_of_all_its_messages() {
        let (_, b) = first_sync();
        let a = item(&b, A);
        assert_eq!(a.title, "Acme exec call today");
        assert_eq!(a.url.clone().unwrap(), format!("https://mail.google.com/mail/u/0/#all/{A}"));
        let texts: Vec<(&str, &str)> = a.messages.iter().map(|m| (m.author.as_str(), m.text.as_str())).collect();
        // Read or not, every message is in; the quoted reply is cut, plain text wins over HTML.
        assert_eq!(texts.len(), 3);
        assert_eq!(
            texts[1..],
            [("Dana Kim", "Can someone senior join a call today?\nWe're seeing delays again."), ("Lee, Sam", "I can take it at 16:15.")]
        );
        assert_eq!(a.messages[1].at, utc(14, 22));
        assert_eq!(a.ended_at.clone(), Some(utc(14, 41)));
        assert_eq!(a.msg_count, Some(3));
        let author = a.author.as_ref().unwrap();
        assert_eq!((author.handle.as_str(), author.display_name.as_str()), ("sam.lee@acme.com", "Lee, Sam"));
        assert_eq!(a.raw_json["msgs"], json!(["18f0a1b2c3d4e5f6", "18f0a1b2c3d4e601", "18f0a1b2c3d4e602"]));
        assert_eq!(a.summary.as_deref(), Some("I can take it at 16:15."));
    }

    #[test]
    fn no_reply_senders_are_told_apart_by_name() {
        let from = |f: &str| sender(&json!({ "payload": { "headers": [{ "name": "From", "value": f }] } })).unwrap().handle;
        assert_eq!(from("\"Alan Ho (Google Sheets)\" <comments-noreply@docs.google.com>"), "comments-noreply@docs.google.com Alan Ho (Google Sheets)");
        assert_ne!(from("\"Xuan Lam (Google Sheets)\" <comments-noreply@docs.google.com>"), from("\"Alan Ho (Google Sheets)\" <comments-noreply@docs.google.com>"));
        assert_eq!(from("Dana Kim <Dana@Acme.com>"), "dana@acme.com");
        assert_eq!(from("\"Luke Gain (Jira)\" <jira@acme.atlassian.net>"), "jira@acme.atlassian.net Luke Gain (Jira)");
        assert_eq!(from("dan <notifications@github.com>"), "notifications@github.com dan");
        assert_eq!(from("'Datadog Alerting' via Devops <devops@acme.com>"), "devops@acme.com 'Datadog Alerting' via Devops");
        assert_eq!(from("\"Ravi Tadi (via Google Drive)\" <ravi@acme.com>"), "ravi@acme.com", "a person's own address stays theirs");
    }

    #[test]
    fn html_only_mail_is_stripped_and_attachments_skipped() {
        let (_, b) = first_sync();
        let text = &item(&b, B).messages[0].text;
        assert_eq!(text, "Hi all,\nPlease book a 10-minute slot & bring Q3 numbers.\nThanks,\nAna's team");
        let c = item(&b, C);
        assert_eq!(c.messages[0].text, "Draft agenda attached.");
        assert_eq!(c.author.as_ref().unwrap().display_name, "Priya");
        assert_eq!(c.title, "Offsite agenda");
    }

    #[test]
    fn history_bumps_new_mail_ignores_read_state_and_drops_threads_that_left_the_inbox() {
        let (_, first) = first_sync();
        let http = google();
        http.on("GET", "/history?startHistoryId=4201", 200, fixture!("history_page1.json"));
        http.on("GET", "pageToken=15432", 200, fixture!("history_page2.json"));
        http.on("GET", &format!("/threads/{A}?"), 200, fixture!("thread_a_new.json"));
        http.on("GET", &format!("/threads/{B}?"), 200, fixture!("thread_b_read.json"));
        http.on("GET", &format!("/threads/{C}?"), 200, fixture!("thread_c_archived.json"));
        let b = poll(&http, first.cursor.clone());
        let g = gets(&http);
        assert_eq!(g[0], format!("{API}/history?startHistoryId=4201&maxResults=500"));
        // Read-state and user-label changes, and untracked sent mail, aren't read.
        assert_eq!(g.iter().filter(|u| u.contains("/threads/")).count(), 2);
        let a = item(&b, A);
        assert!(a.bump);
        assert_eq!(a.messages.len(), 4);
        assert_eq!(a.author.as_ref().unwrap().display_name, "Dana Kim");
        assert_eq!(b.items.len(), 1);
        assert_eq!(b.seen, [C], "archived in Gmail: out of Incoming");
        let c = cursor(&b);
        assert_eq!(c["history_id"], "4250");
        assert_eq!(c["tracked"].as_object().unwrap().keys().collect::<Vec<_>>(), [A, B]);
    }

    #[test]
    fn expired_history_id_relists_and_drops_threads_that_left_the_inbox_meanwhile() {
        let (_, first) = first_sync();
        let mut cur = cursor(&first);
        cur["history_id"] = json!("12");
        cur["tracked"]["18f0000000000old"] = json!(1);
        let http = google();
        full_list(&http);
        http.on("GET", "/history?startHistoryId=12", 404, fixture!("not_found.json"));
        http.on("GET", &format!("/threads/{C}?"), 404, fixture!("not_found.json"));
        let b = poll(&http, Some(cur.to_string()));
        assert_eq!(b.seen, ["18f0000000000old", C]);
        assert!(cursor(&b)["tracked"].get("18f0000000000old").is_none());
        assert!(b.items.iter().all(|i| !i.bump), "nothing new after a re-list");
        assert_eq!(cursor(&b)["history_id"], "4201");
    }

    #[test]
    fn big_queues_carry_over_and_rate_limits_keep_the_rest() {
        let ids: Vec<String> = (0..FETCH_PER_POLL + 5).map(|i| format!("19{i:014x}")).collect();
        let cur = json!({ "history_id": "4300", "queue": ids, "tracked": {} });
        let http = google();
        http.on("GET", "/history?", 200, fixture!("history_empty.json"));
        http.on("GET", "/threads/", 200, fixture!("thread_b.json"));
        let b = poll(&http, Some(cur.to_string()));
        assert_eq!(b.items.len(), FETCH_PER_POLL);
        assert_eq!(cursor(&b)["queue"], json!(ids[FETCH_PER_POLL..]));

        let http = google();
        http.on("GET", "/history?", 200, fixture!("history_empty.json"));
        http.on("GET", "/threads/", 200, fixture!("thread_b.json"));
        http.on("GET", &format!("/threads/{}?", ids[FETCH_PER_POLL + 1]), 429, "{}");
        let b = poll(&http, b.cursor.clone());
        assert_eq!(b.items.len(), 1);
        assert_eq!(cursor(&b)["queue"], json!(ids[FETCH_PER_POLL + 1..]));
    }

    fn send(http: &FakeHttp, op: &str, payload: Value) -> anyhow::Result<Value> {
        let src = source(None);
        let secret = secrets;
        let ctx = Ctx { source: &src, http, secret: &secret, now: crate::sync::connectors::gcal::tests::now() };
        Gmail.send(&ctx, op, &payload)
    }

    fn posts(http: &FakeHttp) -> Vec<(String, Option<Value>)> {
        let base = format!("{API}/threads/");
        http.requests()
            .into_iter()
            .filter(|r| r.0 == "POST" && r.1.starts_with(&base))
            .map(|r| (r.1[base.len()..].to_string(), r.2.map(|b| serde_json::from_str(&b).unwrap())))
            .collect()
    }

    #[test]
    fn file_labels_and_marks_read_unless_new_mail_came() {
        let http = google();
        http.on("GET", &format!("/threads/{A}?format=minimal"), 200, fixture!("thread_a.json"));
        http.on("GET", "/labels", 200, r#"{"labels": [{"id": "Label_3", "name": "Vendors"}, {"id": "Label_9", "name": "Customers"}]}"#);
        http.on("POST", &format!("/threads/{A}/modify"), 200, fixture!("modify.json"));
        let known = json!(["18f0a1b2c3d4e5f6", "18f0a1b2c3d4e601", "18f0a1b2c3d4e602"]);
        let r = send(&http, "gmail.file", json!({ "thread_id": A, "message_ids": known, "label": "Customers" })).unwrap();
        assert_eq!(r["thread_id"], A);
        assert_eq!(posts(&http), [(format!("{A}/modify"), Some(json!({ "removeLabelIds": ["UNREAD"], "addLabelIds": ["Label_9"] })))]);

        let http = google();
        http.on("GET", &format!("/threads/{A}?format=minimal"), 200, fixture!("thread_a.json"));
        let r = send(&http, "gmail.file", json!({ "thread_id": A, "message_ids": ["18f0a1b2c3d4e601"] })).unwrap();
        assert!(r["left_unread"].is_string());
        assert!(posts(&http).is_empty());

        let http = google();
        http.on("GET", "/labels", 200, r#"{"labels": []}"#);
        let e = send(&http, "gmail.file", json!({ "thread_id": A, "label": "Customers" })).unwrap_err().to_string();
        assert!(e.contains("no Gmail label named 'Customers'"), "{e}");
    }

    #[test]
    fn archive_trash_and_restore() {
        let http = google();
        http.on("POST", "/threads/", 200, fixture!("modify.json"));
        send(&http, "gmail.archive", json!({ "thread_id": A })).unwrap();
        send(&http, "gmail.trash", json!({ "thread_id": A })).unwrap();
        send(&http, "gmail.restore", json!({ "thread_id": A })).unwrap();
        assert_eq!(
            posts(&http),
            [
                (format!("{A}/modify"), Some(json!({ "removeLabelIds": ["INBOX"] }))),
                (format!("{A}/trash"), None),
                (format!("{A}/untrash"), None),
                (format!("{A}/modify"), Some(json!({ "addLabelIds": ["INBOX"] }))),
            ]
        );
    }

    #[test]
    fn triage_files_archives_trashes_and_restores() {
        let conn = crate::db::open_in_memory().unwrap();
        let cfg = crate::settings::SourceConfig {
            kind: "gmail".into(),
            tag: "work".into(),
            name: "Work mail".into(),
            enabled: true,
            poll_secs: 120,
            config: json!({ "category_labels": { "customers": "Customers" } }),
        };
        crate::sync::register_sources(&conn, &[cfg]).unwrap();
        let src = crate::sync::load_source(&conn, 1).unwrap();
        let (_, first) = first_sync();
        assert_eq!(crate::sync::apply(&conn, &src, &first).unwrap(), 3);
        conn.execute("INSERT INTO categories (key, label, color, position) VALUES ('customers', 'Customers', '#fff', 9)", []).unwrap();
        conn.execute("INSERT INTO topics (id, name, category) VALUES (1, 'Acme', 'customers'), (2, 'Hiring', 'work')", []).unwrap();
        let id = |ext: &str| -> i64 { conn.query_row("SELECT id FROM items WHERE external_id = ?1", [ext], |r| r.get(0)).unwrap() };
        let ops = || -> Vec<(String, Value)> {
            conn.prepare("SELECT op, payload_json FROM outbox ORDER BY id")
                .unwrap()
                .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
                .unwrap()
                .map(|p| {
                    let (op, p) = p.unwrap();
                    (op, serde_json::from_str(&p).unwrap())
                })
                .collect()
        };
        let incoming = crate::queries::incoming::dismiss;
        incoming(&conn, id(C), true).unwrap();
        incoming(&conn, id(C), false).unwrap();
        crate::queries::incoming::trash(&conn, id(C)).unwrap();
        conn.execute("INSERT INTO topic_links (item_id, topic_id, state) VALUES (?1, 1, 'suggested')", [id(A)]).unwrap();
        crate::queries::incoming::confirm(&conn, id(A), 1).unwrap();
        crate::queries::incoming::move_to(&conn, id(B), 2, None).unwrap();
        let t = |op: &str, p: Value| (op.to_string(), p);
        assert_eq!(
            ops(),
            [
                t("gmail.archive", json!({ "thread_id": C })),
                t("gmail.restore", json!({ "thread_id": C })),
                t("gmail.trash", json!({ "thread_id": C })),
                t("gmail.file", json!({ "thread_id": A, "message_ids": ["18f0a1b2c3d4e5f6", "18f0a1b2c3d4e601", "18f0a1b2c3d4e602"], "label": "Customers" })),
                t("gmail.file", json!({ "thread_id": B, "message_ids": [B] })),
            ]
        );
        let tx = conn.unchecked_transaction().unwrap();
        let msgs = crate::queries::split::message_ids(&tx, id(A)).unwrap();
        assert!(crate::queries::split::split_off(&tx, id(A), &msgs[..1]).is_err(), "email threads move as a whole");
    }
}
