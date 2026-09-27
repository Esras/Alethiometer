//! Thin Slack Web API client over `Http`: bearer auth, form-encoded params,
//! `ok:false` as an error, and Retry-After on 429.
//!
//! Slack answers most failures with HTTP 200 and `{"ok": false, "error": "..."}`,
//! so a 2xx status alone means nothing; `call` checks `ok`.

use crate::sync::http::Http;
use serde_json::Value;
use std::time::Duration;

pub const BASE: &str = "https://slack.com/api/";

/// Retries after a 429 before giving up.
const MAX_RETRIES: u32 = 3;
/// Longest Retry-After we are willing to sleep through; beyond that the poll
/// fails and the next one tries again.
const MAX_WAIT_SECS: u64 = 120;

pub struct Client<'a> {
    pub http: &'a dyn Http,
    pub token: String,
    /// Injectable so tests don't sleep.
    pub sleep: fn(Duration),
}

impl<'a> Client<'a> {
    pub fn new(http: &'a dyn Http, token: String) -> Client<'a> {
        Client { http, token, sleep: std::thread::sleep }
    }

    /// Read method: GET with a query string.
    pub fn get(&self, method: &str, params: &[(&str, &str)]) -> anyhow::Result<Value> {
        let url = if params.is_empty() { format!("{BASE}{method}") } else { format!("{BASE}{method}?{}", form(params)) };
        self.call(method, "GET", &url, None)
    }

    /// Write method: POST, form-encoded (every Web API method accepts this).
    pub fn post(&self, method: &str, params: &[(&str, &str)]) -> anyhow::Result<Value> {
        let url = format!("{BASE}{method}");
        self.call(method, "POST", &url, Some(form(params)))
    }

    fn call(&self, method: &str, verb: &str, url: &str, body: Option<String>) -> anyhow::Result<Value> {
        let auth = format!("Bearer {}", self.token);
        let mut headers = vec![("authorization", auth.as_str())];
        if body.is_some() {
            headers.push(("content-type", "application/x-www-form-urlencoded"));
        }
        let mut attempt = 0;
        loop {
            let resp = self.http.request(verb, url, &headers, body.as_deref())?;
            if resp.status == 429 {
                let wait = resp.header("retry-after").and_then(|v| v.trim().parse::<u64>().ok()).unwrap_or(30);
                if attempt >= MAX_RETRIES || wait > MAX_WAIT_SECS {
                    anyhow::bail!("Slack {method}: rate limited (retry after {wait}s)");
                }
                attempt += 1;
                (self.sleep)(Duration::from_secs(wait));
                continue;
            }
            // Never echo the URL or headers: they may carry the token.
            let v: Value = resp.ok().map_err(|e| anyhow::anyhow!("Slack {method}: {e}"))?.json()?;
            if v.get("ok").and_then(Value::as_bool) != Some(true) {
                let err = v.get("error").and_then(Value::as_str).unwrap_or("unknown error");
                let needed = v.get("needed").and_then(Value::as_str).map(|n| format!(" (needs scope {n})")).unwrap_or_default();
                anyhow::bail!("Slack {method}: {err}{needed}");
            }
            return Ok(v);
        }
    }

    /// Follow `response_metadata.next_cursor` and collect `field` from every
    /// page, up to `max_pages`.
    pub fn get_paged(&self, method: &str, params: &[(&str, &str)], field: &str, max_pages: usize) -> anyhow::Result<Vec<Value>> {
        let mut out = vec![];
        let mut cursor = String::new();
        for _ in 0..max_pages {
            let mut p = params.to_vec();
            if !cursor.is_empty() {
                p.push(("cursor", cursor.as_str()));
            }
            let v = self.get(method, &p)?;
            if let Some(arr) = v.get(field).and_then(Value::as_array) {
                out.extend(arr.iter().cloned());
            }
            cursor = v
                .pointer("/response_metadata/next_cursor")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned();
            if cursor.is_empty() {
                break;
            }
        }
        Ok(out)
    }
}

/// `application/x-www-form-urlencoded` (also used for query strings).
pub fn form(params: &[(&str, &str)]) -> String {
    params.iter().map(|(k, v)| format!("{}={}", enc(k), enc(v))).collect::<Vec<_>>().join("&")
}

fn enc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Slack message timestamps ("1695000000.123456") as seconds.
pub fn ts_secs(ts: &str) -> f64 {
    ts.parse().unwrap_or(0.0)
}

/// Compare two Slack timestamps. They are fixed-format decimals; numeric
/// comparison avoids string-length surprises.
pub fn ts_after(a: &str, b: &str) -> bool {
    let (ai, af) = split_ts(a);
    let (bi, bf) = split_ts(b);
    (ai, af) > (bi, bf)
}

fn split_ts(ts: &str) -> (u64, u64) {
    let mut it = ts.splitn(2, '.');
    let i = it.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    let f = it.next().map(|s| format!("{s:0<6}")).and_then(|s| s[..6].parse().ok()).unwrap_or(0);
    (i, f)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_forms() {
        assert_eq!(form(&[("text", "a b&c=d/é"), ("channel", "C1")]), "text=a%20b%26c%3Dd%2F%C3%A9&channel=C1");
    }

    #[test]
    fn compares_ts() {
        assert!(ts_after("1695000000.000200", "1695000000.000100"));
        assert!(ts_after("1695000001.0001", "1695000000.999999"));
        assert!(!ts_after("1695000000.000100", "1695000000.000100"));
    }
}
