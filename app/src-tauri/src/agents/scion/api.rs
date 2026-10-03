//! Scion Hub REST calls (`/api/v1`, bearer UAT). Shapes follow the Go JSON
//! tags in `pkg/hub` and `pkg/store`; see `tests/fixtures/scion/README.md`.

use crate::sync::http::Http;
use serde_json::{json, Value};

/// A non-2xx answer, from the Hub's `{"error": {"code", "message"}}` envelope.
#[derive(Debug)]
pub struct HubError {
    pub status: u16,
    pub code: String,
    pub message: String,
}

impl std::fmt::Display for HubError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match (self.status, self.code.as_str()) {
            (401, _) => write!(f, "the Hub rejected the access token ({})", self.message),
            (_, "") => write!(f, "HTTP {}: {}", self.status, self.message),
            _ => write!(f, "{} ({})", self.message, self.code),
        }
    }
}

impl std::error::Error for HubError {}

/// The Hub error behind `e`, if it was one.
pub fn hub_error(e: &anyhow::Error) -> Option<&HubError> {
    e.downcast_ref::<HubError>()
}

pub struct Client<'a> {
    pub http: &'a dyn Http,
    pub base: String,
    pub token: String,
}

/// Percent-encode a path segment or query value.
pub fn enc(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.~:@".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

impl Client<'_> {
    pub fn call(&self, method: &str, path: &str, body: Option<&Value>) -> anyhow::Result<Value> {
        let url = format!("{}{path}", self.base.trim_end_matches('/'));
        let auth = format!("Bearer {}", self.token);
        let mut headers = vec![("authorization", auth.as_str()), ("accept", "application/json")];
        let text = body.map(|b| b.to_string());
        if text.is_some() {
            headers.push(("content-type", "application/json"));
        }
        let r = self.http.request(method, &url, &headers, text.as_deref())?;
        let v: Value = serde_json::from_str(&r.body).unwrap_or(Value::Null);
        if !(200..300).contains(&r.status) {
            let e = &v["error"];
            let message = e["message"].as_str().map(str::to_owned).unwrap_or_else(|| r.body.chars().take(300).collect());
            return Err(HubError { status: r.status, code: e["code"].as_str().unwrap_or("").into(), message }.into());
        }
        Ok(v)
    }

    pub fn get(&self, path: &str) -> anyhow::Result<Value> {
        self.call("GET", path, None)
    }

    pub fn post(&self, path: &str, body: &Value) -> anyhow::Result<Value> {
        self.call("POST", path, Some(body))
    }

    /// A cursor-paged list (`nextCursor`), up to `pages` pages.
    pub fn list(&self, path: &str, key: &str, pages: usize) -> anyhow::Result<Vec<Value>> {
        let mut out = vec![];
        let mut cursor = String::new();
        for _ in 0..pages {
            let sep = if path.contains('?') { '&' } else { '?' };
            let p = if cursor.is_empty() { path.to_owned() } else { format!("{path}{sep}cursor={}", enc(&cursor)) };
            let v = self.get(&p)?;
            out.extend(v[key].as_array().cloned().unwrap_or_default());
            match v["nextCursor"].as_str() {
                Some(c) if !c.is_empty() => cursor = c.to_owned(),
                _ => break,
            }
        }
        Ok(out)
    }

    pub fn me(&self) -> anyhow::Result<Value> {
        self.get("/api/v1/auth/me")
    }

    pub fn projects(&self) -> anyhow::Result<Vec<Value>> {
        self.list("/api/v1/projects?limit=100", "projects", 5)
    }

    pub fn agents(&self, project: &str) -> anyhow::Result<Vec<Value>> {
        self.list(&format!("/api/v1/projects/{}/agents?limit=500", enc(project)), "agents", 4)
    }

    /// Your conversations (DMs, groups you're in) plus every group thread in `project`.
    pub fn conversations(&self, project: &str) -> anyhow::Result<Vec<Value>> {
        let v = self.get(&format!("/api/v1/conversations?include_project_groups={}&limit=200", enc(project)))?;
        Ok(v["conversations"].as_array().cloned().unwrap_or_default())
    }

    /// One conversation with its participants.
    pub fn conversation(&self, id: &str) -> anyhow::Result<Value> {
        self.get(&format!("/api/v1/conversations/{}", enc(id)))
    }

    /// Newest first, as the Hub returns them.
    pub fn messages(&self, conv: &str, limit: i64) -> anyhow::Result<Vec<Value>> {
        let v = self.get(&format!("/api/v1/conversations/{}/messages?limit={limit}", enc(conv)))?;
        Ok(v["items"].as_array().cloned().unwrap_or_default())
    }

    /// Agent ↔ agent messages of one agent (native chat; needs attach on it).
    pub fn interagent(&self, dm_key: &str, limit: i64) -> anyhow::Result<Vec<Value>> {
        let v = self.get(&format!("/api/v1/chat/conversations/{}/interagent?limit={limit}", enc(dm_key)))?;
        Ok(v["messages"].as_array().cloned().unwrap_or_default())
    }

    /// Post to a group thread as you. Routing is the Hub's: @mentions, else the thread's default agent.
    pub fn post_thread(&self, topic: &str, content: &str, reply_to: Option<&str>, idempotency: &str) -> anyhow::Result<Value> {
        let mut body = json!({ "content": content, "idempotency_key": idempotency });
        if let Some(r) = reply_to {
            body["reply_to_id"] = json!(r);
        }
        self.post(&format!("/api/v1/chat/conversations/{}/messages", enc(topic)), &body)
    }

    /// A DM from you to an agent.
    pub fn message_agent(&self, agent: &str, text: &str, kind: &str, wake: bool) -> anyhow::Result<Value> {
        let body = json!({ "structured_message": { "msg": text, "type": kind }, "wake": wake });
        self.post(&format!("/api/v1/agents/{}/message", enc(agent)), &body)
    }

    pub fn action(&self, agent: &str, action: &str) -> anyhow::Result<Value> {
        self.post(&format!("/api/v1/agents/{}/{action}", enc(agent)), &json!({}))
    }

    pub fn keys(&self, agent: &str, keys: &str) -> anyhow::Result<Value> {
        self.post(&format!("/api/v1/agents/{}/keys", enc(agent)), &json!({ "keys": keys }))
    }

    pub fn agent(&self, agent: &str) -> anyhow::Result<Value> {
        self.get(&format!("/api/v1/agents/{}", enc(agent)))
    }

    pub fn create_agent(&self, body: &Value) -> anyhow::Result<Value> {
        self.post("/api/v1/agents", body)
    }

    pub fn add_participant(&self, conv: &str, agent: &str) -> anyhow::Result<Value> {
        self.post(&format!("/api/v1/conversations/{}/participants", enc(conv)), &json!({ "principalKind": "agent", "principalId": agent }))
    }

    pub fn templates(&self) -> anyhow::Result<Vec<Value>> {
        self.list("/api/v1/templates?limit=100", "templates", 3)
    }

    pub fn template_file(&self, id: &str, path: &str) -> anyhow::Result<String> {
        let v = self.get(&format!("/api/v1/templates/{}/files/{path}", enc(id)))?;
        Ok(v["content"].as_str().unwrap_or("").to_owned())
    }

    pub fn harness_configs(&self) -> anyhow::Result<Vec<Value>> {
        self.list("/api/v1/harness-configs?limit=100", "harnessConfigs", 3)
    }

    /// The brokers that run agents for a project.
    pub fn providers(&self, project: &str) -> anyhow::Result<Vec<Value>> {
        let v = self.get(&format!("/api/v1/projects/{}/providers", enc(project)))?;
        Ok(v["providers"].as_array().cloned().unwrap_or_default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync::http::FakeHttp;

    #[test]
    fn errors_read_the_hub_envelope() {
        let http = FakeHttp::default();
        http.on("POST", "/message", 409, include_str!("../../../tests/fixtures/scion/agent_not_running.json"));
        let c = Client { http: &http, base: "https://hub/".into(), token: "scion_pat_x".into() };
        let e = c.message_agent("a1", "hi", "instruction", false).unwrap_err();
        let h = hub_error(&e).unwrap();
        assert_eq!((h.status, h.code.as_str()), (409, "agent_not_running"));
        assert!(e.to_string().contains("is stopped"), "{e}");
        let headers = &http.header_log.lock().unwrap()[0];
        assert!(headers.iter().any(|(k, v)| k == "authorization" && v == "Bearer scion_pat_x"));
        assert_eq!(http.requests()[0].1, "https://hub/api/v1/agents/a1/message");
    }

    #[test]
    fn lists_follow_the_cursor() {
        let http = FakeHttp::default();
        http.on("GET", "/api/v1/templates?limit=100", 200, r#"{"templates":[{"name":"a"}],"nextCursor":"c+1"}"#);
        http.on("GET", "cursor=c%2B1", 200, r#"{"templates":[{"name":"b"}]}"#);
        let c = Client { http: &http, base: "https://hub".into(), token: "t".into() };
        let t = c.templates().unwrap();
        assert_eq!(t.len(), 2);
        assert_eq!(http.requests().len(), 2);
    }
}
