//! A small Microsoft Graph client: bearer auth, JSON, `@odata.nextLink`
//! paging, and 429/503 with `Retry-After`.

use crate::sync::http::Http;
use serde_json::Value;
use std::time::Duration;

pub const BASE: &str = "https://graph.microsoft.com/v1.0";
/// A 429 or 503 is retried this many times, waiting `Retry-After`.
const MAX_RETRIES: u32 = 3;
/// Longer waits fail the call; the next poll tries again.
const MAX_WAIT_SECS: u64 = 120;

/// Graph's error body: `{"error": {"code", "message"}}`.
#[derive(Debug)]
pub struct GraphError {
    pub status: u16,
    pub code: String,
    pub message: String,
}

impl std::fmt::Display for GraphError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Teams: HTTP {} {}", self.status, self.code)?;
        if !self.message.is_empty() {
            write!(f, ": {}", self.message)?;
        }
        if self.status == 403 {
            write!(
                f,
                " (missing permission? Connect again after changing what Teams reads)"
            )?;
        }
        Ok(())
    }
}

impl std::error::Error for GraphError {}

impl GraphError {
    /// The sign-in itself is bad: no point reading other streams.
    pub fn is_fatal(&self) -> bool {
        self.status == 401
    }

    pub fn is_not_found(&self) -> bool {
        self.status == 404
    }
}

pub struct Client<'a> {
    pub http: &'a dyn Http,
    pub token: String,
    /// Injectable so tests don't sleep.
    pub sleep: fn(Duration),
}

impl<'a> Client<'a> {
    pub fn new(http: &'a dyn Http, token: String) -> Client<'a> {
        Client {
            http,
            token,
            sleep: std::thread::sleep,
        }
    }

    /// GET a path under `BASE` ("/me/chats?…") or a full URL (a nextLink).
    pub fn get(&self, path_or_url: &str) -> anyhow::Result<Value> {
        self.call("GET", path_or_url, None)
    }

    pub fn post(&self, path: &str, body: &Value) -> anyhow::Result<Value> {
        self.call("POST", path, Some(body.to_string()))
    }

    /// Every page's `value`, following `@odata.nextLink`, up to `max_pages`.
    /// `stop` sees each page and ends the paging when it returns true.
    pub fn get_paged(
        &self,
        path: &str,
        max_pages: usize,
        mut stop: impl FnMut(&[Value]) -> bool,
    ) -> anyhow::Result<Vec<Value>> {
        let mut out = vec![];
        let mut next = Some(path.to_owned());
        for _ in 0..max_pages {
            let Some(url) = next.take() else { break };
            let v = self.get(&url)?;
            let page: Vec<Value> = v.get("value").and_then(Value::as_array).cloned().unwrap_or_default();
            let done = stop(&page);
            out.extend(page);
            if done {
                break;
            }
            next = v.get("@odata.nextLink").and_then(Value::as_str).map(str::to_owned);
        }
        Ok(out)
    }

    fn call(&self, verb: &str, path_or_url: &str, body: Option<String>) -> anyhow::Result<Value> {
        let url = if path_or_url.starts_with("https://") {
            path_or_url.to_owned()
        } else {
            format!("{BASE}{path_or_url}")
        };
        let auth = format!("Bearer {}", self.token);
        let mut headers = vec![("authorization", auth.as_str()), ("accept", "application/json")];
        if body.is_some() {
            headers.push(("content-type", "application/json"));
        }
        let mut attempt = 0;
        loop {
            let resp = self.http.request(verb, &url, &headers, body.as_deref())?;
            if resp.status == 429 || resp.status == 503 {
                let wait = resp
                    .header("retry-after")
                    .and_then(|v| v.trim().parse::<u64>().ok())
                    .unwrap_or(10);
                if attempt >= MAX_RETRIES || wait > MAX_WAIT_SECS {
                    anyhow::bail!("Teams: throttled (retry after {wait}s)");
                }
                attempt += 1;
                (self.sleep)(Duration::from_secs(wait));
                continue;
            }
            // Never echo the URL or headers: the headers carry the token.
            if !(200..300).contains(&resp.status) {
                let v: Value = serde_json::from_str(&resp.body).unwrap_or(Value::Null);
                let s = |k: &str| {
                    v.pointer(&format!("/error/{k}"))
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_owned()
                };
                return Err(GraphError {
                    status: resp.status,
                    code: s("code"),
                    message: s("message").chars().take(300).collect(),
                }
                .into());
            }
            if resp.body.trim().is_empty() {
                // 204 No Content (markChatReadForUser, setReaction).
                return Ok(Value::Null);
            }
            return Ok(resp.json()?);
        }
    }
}

/// Percent-encode a path segment or query value.
pub fn enc(s: &str) -> String {
    super::super::gcal::enc(s)
}

/// Message ids are Teams' creation times in milliseconds: compare as numbers
/// (`docs/chat.md`), as text when one isn't a number.
pub fn key_after(a: &str, b: &str) -> bool {
    match (a.parse::<u64>(), b.parse::<u64>()) {
        (Ok(x), Ok(y)) => x > y,
        _ => a > b,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync::http::{FakeHttp, Response};

    #[test]
    fn keys_compare_as_numbers() {
        assert!(key_after("1700000000100", "999999999999"));
        assert!(!key_after("1700000000100", "1700000000100"));
    }

    #[test]
    fn throttling_waits_then_errors_carry_graphs_code() {
        let http = FakeHttp::default();
        http.on_response(
            "GET",
            "/me",
            Response {
                status: 429,
                body: String::new(),
                headers: vec![("retry-after".into(), "2".into())],
            },
        );
        let mut c = Client::new(&http, "t".into());
        c.sleep = |_| {};
        let e = c.get("/me").unwrap_err().to_string();
        assert!(e.contains("throttled"), "{e}");
        assert_eq!(http.requests().len(), 1 + MAX_RETRIES as usize);

        http.on(
            "GET",
            "/me",
            403,
            r#"{"error":{"code":"Forbidden","message":"Missing scope permissions on the request."}}"#,
        );
        let e = c.get("/me").unwrap_err();
        let g = e.downcast_ref::<GraphError>().unwrap();
        assert_eq!((g.status, g.code.as_str()), (403, "Forbidden"));
        assert!(!e.to_string().contains("Bearer"));
    }
}
