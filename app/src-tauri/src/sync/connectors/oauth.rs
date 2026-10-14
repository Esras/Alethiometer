//! What the OAuth sign-ins (Google's, Microsoft's) share: PKCE, the loopback
//! listener that takes the browser's redirect, the sign-ins in progress, and
//! the in-memory access tokens. Each provider keeps its own URLs, token
//! requests and `auth_start`/`auth_status` (`gcal::oauth`, `teams::oauth`).
//!
//! Only refresh tokens are persisted, as secrets. Access tokens live in
//! process memory and are refreshed shortly before they expire (or after a 401).
//! Consent is two API calls because `api::call` only lends us `&App`, which a
//! background thread can't keep: `auth_start` returns the consent URL and
//! starts a listener that exchanges the code; `auth_status` reports progress
//! and moves the refresh token into the secret store once the exchange is done.

use base64::Engine;
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// How long the loopback listener waits for the browser.
pub const CONSENT_TIMEOUT: Duration = Duration::from_secs(600);
/// Refresh this long before the provider's stated expiry.
pub const EXPIRY_MARGIN: Duration = Duration::from_secs(60);

// ---------------------------------------------------------------- access tokens

struct Cached {
    token: String,
    expires: Instant,
}

/// In-memory access tokens, keyed by secret prefix plus a fingerprint of the
/// refresh token (so a fresh consent never reuses a stale access token).
#[derive(Default)]
pub struct TokenCache(Mutex<BTreeMap<String, Cached>>);

impl TokenCache {
    pub const fn new() -> TokenCache {
        TokenCache(Mutex::new(BTreeMap::new()))
    }

    pub fn get(&self, key: &str) -> Option<String> {
        let map = self.0.lock().unwrap_or_else(|e| e.into_inner());
        map.get(key)
            .filter(|c| c.expires > Instant::now())
            .map(|c| c.token.clone())
    }

    pub fn put(&self, key: &str, token: &str, expires: Instant) {
        let mut map = self.0.lock().unwrap_or_else(|e| e.into_inner());
        map.insert(
            key.to_string(),
            Cached {
                token: token.to_string(),
                expires,
            },
        );
    }

    pub fn forget(&self, key: &str) {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).remove(key);
    }

    pub fn forget_prefix(&self, prefix: &str) {
        let start = format!("{prefix}:");
        self.0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .retain(|k, _| !k.starts_with(&start));
    }
}

/// Process-wide cache used by the connectors.
pub static CACHE: TokenCache = TokenCache::new();

pub fn cache_key(prefix: &str, refresh_token: &str) -> String {
    format!("{prefix}:{:016x}", super::gcal::fnv(refresh_token.as_bytes()))
}

// ---------------------------------------------------------------- PKCE and forms

pub fn random_b64(n: usize) -> anyhow::Result<String> {
    use ring::rand::SecureRandom;
    let mut buf = vec![0u8; n];
    ring::rand::SystemRandom::new()
        .fill(&mut buf)
        .map_err(|_| anyhow::anyhow!("no system randomness"))?;
    Ok(base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(buf))
}

/// RFC 7636 S256: base64url(sha256(verifier)), no padding.
pub fn pkce_challenge(verifier: &str) -> String {
    let d = ring::digest::digest(&ring::digest::SHA256, verifier.as_bytes());
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(d.as_ref())
}

pub fn form(pairs: &[(&str, &str)]) -> String {
    pairs
        .iter()
        .map(|(k, v)| format!("{}={}", super::gcal::enc(k), super::gcal::enc(v)))
        .collect::<Vec<_>>()
        .join("&")
}

/// Percent-decoding as in a query string (`+` is a space).
pub fn pct_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < b.len() => {
                let hex = |c: u8| (c as char).to_digit(16);
                match (hex(b[i + 1]), hex(b[i + 2])) {
                    (Some(h), Some(l)) => {
                        out.push((h * 16 + l) as u8);
                        i += 2;
                    }
                    _ => out.push(b'%'),
                }
            }
            c => out.push(c),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

// ---------------------------------------------------------------- loopback listener

/// What came back on the redirect.
#[derive(Debug, PartialEq)]
pub enum Callback {
    Code(String),
    Denied(String),
    /// Not our redirect (favicon, wrong state); keep waiting.
    Ignore,
}

/// Parse the request line of a redirect: `GET /?code=…&state=… HTTP/1.1`.
pub fn parse_callback(request_line: &str, expected_state: &str) -> Callback {
    let target = request_line.split_whitespace().nth(1).unwrap_or("");
    let Some((path, query)) = target.split_once('?') else {
        return Callback::Ignore;
    };
    if path != "/" {
        return Callback::Ignore;
    }
    let params: BTreeMap<String, String> = query
        .split('&')
        .filter_map(|kv| kv.split_once('='))
        .map(|(k, v)| (pct_decode(k), pct_decode(v)))
        .collect();
    if params.get("state").map(String::as_str) != Some(expected_state) {
        return Callback::Ignore;
    }
    if let Some(e) = params.get("error") {
        return Callback::Denied(e.clone());
    }
    match params.get("code") {
        Some(c) => Callback::Code(c.clone()),
        None => Callback::Ignore,
    }
}

fn read_request_line(stream: &mut TcpStream) -> String {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let mut buf = Vec::new();
    let mut chunk = [0u8; 1024];
    while buf.len() < 16 * 1024 {
        match stream.read(&mut chunk) {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                buf.extend_from_slice(&chunk[..n]);
                if buf.windows(4).any(|w| w == b"\r\n\r\n") {
                    break;
                }
            }
        }
    }
    String::from_utf8_lossy(&buf).lines().next().unwrap_or("").to_string()
}

fn respond(stream: &mut TcpStream, status: &str, text: &str) {
    let body = format!(
        "<!doctype html><meta charset=utf-8><title>Alethiometer</title><body style=\"font:15px system-ui;background:#1d2021;color:#ebdbb2;padding:3em\"><p>{text}</p>"
    );
    let _ = write!(
        stream,
        "HTTP/1.1 {status}\r\ncontent-type: text/html; charset=utf-8\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
        body.len()
    );
}

/// Accept connections until our redirect arrives or the deadline passes.
/// `exchange` turns the code into a refresh token. `provider` names who
/// signs you in ("Google") on the page the browser shows.
pub fn serve_callback(
    listener: TcpListener,
    deadline: Instant,
    state: &str,
    provider: &str,
    exchange: impl Fn(&str) -> anyhow::Result<String>,
) -> Flow {
    if listener.set_nonblocking(true).is_err() {
        return Flow::Failed("couldn't start the loopback listener".into());
    }
    while Instant::now() < deadline {
        match listener.accept() {
            Ok((mut stream, _)) => {
                let _ = stream.set_nonblocking(false);
                match parse_callback(&read_request_line(&mut stream), state) {
                    Callback::Ignore => respond(&mut stream, "404 Not Found", "Not found."),
                    Callback::Denied(e) => {
                        respond(
                            &mut stream,
                            "200 OK",
                            &format!("{provider} access was not granted. You can close this tab."),
                        );
                        return Flow::Failed(format!("consent denied ({e})"));
                    }
                    Callback::Code(code) => {
                        return match exchange(&code) {
                            Ok(refresh) => {
                                respond(
                                    &mut stream,
                                    "200 OK",
                                    &format!("Alethiometer is connected to {provider}. You can close this tab."),
                                );
                                Flow::Exchanged(refresh)
                            }
                            Err(e) => {
                                respond(
                                    &mut stream,
                                    "200 OK",
                                    "Alethiometer couldn't finish connecting; its sync status says why.",
                                );
                                Flow::Failed(e.to_string())
                            }
                        };
                    }
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => std::thread::sleep(Duration::from_millis(150)),
            Err(e) => return Flow::Failed(format!("loopback listener: {e}")),
        }
    }
    Flow::Failed("timed out waiting for the browser; press Connect again".into())
}

// ---------------------------------------------------------------- sign-ins in progress

pub enum Flow {
    Waiting,
    /// Code exchanged; the refresh token waits here until `auth_status` stores it.
    Exchanged(String),
    Failed(String),
    Connected,
}

/// Sign-ins in progress, by source id.
static FLOWS: Mutex<BTreeMap<i64, Flow>> = Mutex::new(BTreeMap::new());

pub fn flows() -> std::sync::MutexGuard<'static, BTreeMap<i64, Flow>> {
    FLOWS.lock().unwrap_or_else(|e| e.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pkce_challenge_is_s256_base64url() {
        // Computed independently: base64url(sha256(verifier)) without padding.
        assert_eq!(
            pkce_challenge("dBjftJeZ4CVP-mJ92K9mzvXpApYM4XqTANAJfKlLPIo"),
            "5AK02UWhvE-wcLJ2JUX11eybNwnNd84URykhUrBvCDM"
        );
        let v = random_b64(48).unwrap();
        assert_eq!(v.len(), 64);
        assert!(v.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'));
    }

    #[test]
    fn callback_parsing_checks_state() {
        let line = "GET /?state=s%2B1&code=4%2F0AbC&scope=x HTTP/1.1";
        assert_eq!(parse_callback(line, "s+1"), Callback::Code("4/0AbC".into()));
        assert_eq!(parse_callback(line, "other"), Callback::Ignore);
        assert_eq!(parse_callback("GET /favicon.ico HTTP/1.1", "s"), Callback::Ignore);
        assert_eq!(
            parse_callback("GET /?error=access_denied&state=s HTTP/1.1", "s"),
            Callback::Denied("access_denied".into())
        );
    }

    #[test]
    fn loopback_listener_exchanges_the_code() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            serve_callback(
                listener,
                Instant::now() + Duration::from_secs(10),
                "st8",
                "Google",
                |code| {
                    assert_eq!(code, "the-code");
                    Ok("refresh-1".to_string())
                },
            )
        });
        // A stray request first, then the real redirect.
        for path in ["/favicon.ico", "/?code=the-code&state=st8"] {
            let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
            write!(s, "GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n").unwrap();
            let mut out = String::new();
            let _ = s.read_to_string(&mut out);
            assert!(out.starts_with("HTTP/1.1"));
        }
        match server.join().unwrap() {
            Flow::Exchanged(r) => assert_eq!(r, "refresh-1"),
            _ => panic!("expected an exchanged refresh token"),
        }
    }
}
