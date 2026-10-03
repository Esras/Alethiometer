//! Google OAuth 2.0 for a desktop ("installed") app: PKCE plus a loopback
//! redirect on 127.0.0.1. The user brings their own OAuth client (Desktop type).
//!
//! Only the refresh token is persisted, as a secret. Access tokens live in
//! process memory and are refreshed shortly before they expire (or after a 401).
//!
//! Consent is two API calls because `api::call` only lends us `&App`, which a
//! background thread can't keep: `gcal.auth_start` returns the consent URL and
//! starts a listener that exchanges the code; `gcal.auth_status` reports progress
//! and moves the refresh token into the secret store once the exchange is done.

use crate::error::{msg, Result};
use crate::state::App;
use crate::sync::http::{Http, UreqHttp};
use crate::sync::Ctx;
use base64::Engine;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Mutex;
use std::time::{Duration, Instant};

pub const AUTH_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";
pub const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
/// Read, create and change events on calendars the user owns (their primary
/// calendar included). Enough for polling, RSVP and holds; nothing else.
pub const DEFAULT_SCOPE: &str = "https://www.googleapis.com/auth/calendar.events.owned";
/// Read mail and change labels (mark read); no delete, no send.
pub const GMAIL_SCOPE: &str = "https://www.googleapis.com/auth/gmail.modify";

/// How long the loopback listener waits for the browser.
const CONSENT_TIMEOUT: Duration = Duration::from_secs(600);
/// Refresh this long before Google's stated expiry.
const EXPIRY_MARGIN: Duration = Duration::from_secs(60);

/// Secret names for a Google source; Gmail shares the calendar's OAuth client by default.
pub struct SecretNames {
    pub client_id: String,
    pub client_secret: String,
    pub refresh_token: String,
}

impl SecretNames {
    pub fn of(kind: &str, config: &Value) -> SecretNames {
        let prefix = prefix_of(kind, config);
        let default_client = if kind == "gcal" { prefix.as_str() } else { "gcal" };
        let client = config.get("client_prefix").and_then(Value::as_str).unwrap_or(default_client);
        SecretNames {
            client_id: format!("{client}.client_id"),
            client_secret: format!("{client}.client_secret"),
            refresh_token: format!("{prefix}.refresh_token"),
        }
    }
}

pub fn prefix_of(kind: &str, config: &Value) -> String {
    config.get("secret_prefix").and_then(Value::as_str).unwrap_or(kind).to_string()
}

pub fn scopes_of(kind: &str, config: &Value) -> String {
    match config.get("scopes").and_then(Value::as_array) {
        Some(list) if !list.is_empty() => list.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(" "),
        _ if kind == "gmail" => GMAIL_SCOPE.to_string(),
        _ => DEFAULT_SCOPE.to_string(),
    }
}

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

    fn get(&self, key: &str) -> Option<String> {
        let map = self.0.lock().unwrap_or_else(|e| e.into_inner());
        map.get(key).filter(|c| c.expires > Instant::now()).map(|c| c.token.clone())
    }

    pub fn put(&self, key: &str, token: &str, expires: Instant) {
        let mut map = self.0.lock().unwrap_or_else(|e| e.into_inner());
        map.insert(key.to_string(), Cached { token: token.to_string(), expires });
    }

    fn forget(&self, key: &str) {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).remove(key);
    }

    fn forget_prefix(&self, prefix: &str) {
        let start = format!("{prefix}:");
        self.0.lock().unwrap_or_else(|e| e.into_inner()).retain(|k, _| !k.starts_with(&start));
    }
}

/// Process-wide cache used by the connector.
pub static CACHE: TokenCache = TokenCache::new();

pub fn cache_key(prefix: &str, refresh_token: &str) -> String {
    format!("{prefix}:{:016x}", super::fnv(refresh_token.as_bytes()))
}

/// A valid access token for this source, refreshing if needed.
pub fn access_token(ctx: &Ctx, cache: &TokenCache) -> anyhow::Result<String> {
    let (kind, config) = (&ctx.source.kind, &ctx.source.config);
    let names = SecretNames::of(kind, config);
    let refresh = (ctx.secret)(&names.refresh_token).ok_or_else(|| {
        anyhow::anyhow!(
            "'{}' isn't connected to Google yet (no secret '{}'); press Connect or run gcal.auth_start",
            ctx.source.name,
            names.refresh_token
        )
    })?;
    let key = cache_key(&prefix_of(kind, config), &refresh);
    if let Some(t) = cache.get(&key) {
        return Ok(t);
    }
    let client_id = ctx.require_secret(&names.client_id)?;
    let client_secret = ctx.require_secret(&names.client_secret)?;
    let (token, expires_in) = refresh_access_token(ctx.http, &client_id, &client_secret, &refresh)?;
    let life = Duration::from_secs(expires_in).saturating_sub(EXPIRY_MARGIN);
    cache.put(&key, &token, Instant::now() + life);
    Ok(token)
}

/// Drop the cached token after a 401 so the next call refreshes.
pub fn invalidate(ctx: &Ctx, cache: &TokenCache) {
    let names = SecretNames::of(&ctx.source.kind, &ctx.source.config);
    if let Some(refresh) = (ctx.secret)(&names.refresh_token) {
        cache.forget(&cache_key(&prefix_of(&ctx.source.kind, &ctx.source.config), &refresh));
    }
}

fn refresh_access_token(http: &dyn Http, client_id: &str, client_secret: &str, refresh: &str) -> anyhow::Result<(String, u64)> {
    let body = form(&[
        ("client_id", client_id),
        ("client_secret", client_secret),
        ("grant_type", "refresh_token"),
        ("refresh_token", refresh),
    ]);
    let resp = http.request("POST", TOKEN_URL, &[("content-type", "application/x-www-form-urlencoded")], Some(&body))?;
    let v: Value = resp.json().unwrap_or(Value::Null);
    if !(200..300).contains(&resp.status) {
        // The error body is Google's `{error, error_description}`; it never echoes secrets.
        let err = v.get("error").and_then(Value::as_str).unwrap_or("unknown_error");
        if err == "invalid_grant" {
            anyhow::bail!(
                "Google refused the refresh token (expired or revoked; apps in Testing mode get 7-day tokens). Run gcal.auth_start again"
            );
        }
        anyhow::bail!("Google token refresh failed: HTTP {} {err}", resp.status);
    }
    let token = v.get("access_token").and_then(Value::as_str).ok_or_else(|| anyhow::anyhow!("token response had no access_token"))?;
    let expires_in = v.get("expires_in").and_then(Value::as_u64).unwrap_or(3599);
    Ok((token.to_string(), expires_in))
}

/// Exchange an authorization code for a refresh token.
pub fn exchange_code(
    http: &dyn Http,
    client_id: &str,
    client_secret: &str,
    code: &str,
    verifier: &str,
    redirect_uri: &str,
) -> anyhow::Result<String> {
    let body = form(&[
        ("client_id", client_id),
        ("client_secret", client_secret),
        ("code", code),
        ("code_verifier", verifier),
        ("grant_type", "authorization_code"),
        ("redirect_uri", redirect_uri),
    ]);
    let resp = http.request("POST", TOKEN_URL, &[("content-type", "application/x-www-form-urlencoded")], Some(&body))?;
    let v: Value = resp.json().unwrap_or(Value::Null);
    if !(200..300).contains(&resp.status) {
        let err = v.get("error").and_then(Value::as_str).unwrap_or("unknown_error");
        anyhow::bail!("Google code exchange failed: HTTP {} {err}", resp.status);
    }
    v.get("refresh_token").and_then(Value::as_str).map(str::to_owned).ok_or_else(|| {
        anyhow::anyhow!("Google returned no refresh token; remove Alethiometer's access at myaccount.google.com/permissions and try again")
    })
}

// ---------------------------------------------------------------- PKCE and URLs

fn random_b64(n: usize) -> anyhow::Result<String> {
    use ring::rand::SecureRandom;
    let mut buf = vec![0u8; n];
    ring::rand::SystemRandom::new().fill(&mut buf).map_err(|_| anyhow::anyhow!("no system randomness"))?;
    Ok(base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(buf))
}

/// RFC 7636 S256: base64url(sha256(verifier)), no padding.
pub fn pkce_challenge(verifier: &str) -> String {
    let d = ring::digest::digest(&ring::digest::SHA256, verifier.as_bytes());
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(d.as_ref())
}

pub fn consent_url(client_id: &str, redirect_uri: &str, scope: &str, challenge: &str, state: &str, login_hint: Option<&str>) -> String {
    let mut params = vec![
        ("client_id", client_id),
        ("redirect_uri", redirect_uri),
        ("response_type", "code"),
        ("scope", scope),
        ("code_challenge", challenge),
        ("code_challenge_method", "S256"),
        ("state", state),
        // Offline + consent guarantees a refresh token even on a repeat consent.
        ("access_type", "offline"),
        ("prompt", "consent"),
    ];
    if let Some(h) = login_hint {
        params.push(("login_hint", h));
    }
    format!("{AUTH_URL}?{}", form(&params))
}

pub fn form(pairs: &[(&str, &str)]) -> String {
    pairs.iter().map(|(k, v)| format!("{}={}", super::enc(k), super::enc(v))).collect::<Vec<_>>().join("&")
}

fn pct_decode(s: &str) -> String {
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
    let Some((path, query)) = target.split_once('?') else { return Callback::Ignore };
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
/// `exchange` turns the code into a refresh token.
pub fn serve_callback(
    listener: TcpListener,
    deadline: Instant,
    state: &str,
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
                        respond(&mut stream, "200 OK", "Google access was not granted. You can close this tab.");
                        return Flow::Failed(format!("consent denied ({e})"));
                    }
                    Callback::Code(code) => {
                        return match exchange(&code) {
                            Ok(refresh) => {
                                respond(&mut stream, "200 OK", "Alethiometer is connected to Google. You can close this tab.");
                                Flow::Exchanged(refresh)
                            }
                            Err(e) => {
                                respond(&mut stream, "200 OK", "Alethiometer couldn't finish connecting. Check gcal.auth_status.");
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
    Flow::Failed("timed out waiting for the browser; run gcal.auth_start again".into())
}

// ---------------------------------------------------------------- consent API

pub enum Flow {
    Waiting,
    /// Code exchanged; the refresh token waits here until `auth_status` stores it.
    Exchanged(String),
    Failed(String),
    Connected,
}

static FLOWS: Mutex<BTreeMap<i64, Flow>> = Mutex::new(BTreeMap::new());

fn flows() -> std::sync::MutexGuard<'static, BTreeMap<i64, Flow>> {
    FLOWS.lock().unwrap_or_else(|e| e.into_inner())
}

/// `source_id`, else the first source of `kind` (default `gcal`).
fn pick_source(app: &App, args: &Value) -> Result<crate::sync::Source> {
    let id = match crate::queries::opt_i64(args, "source_id") {
        Some(id) => id,
        None => {
            let kind = args.get("kind").and_then(Value::as_str).unwrap_or("gcal");
            app.store()
                .read(|r| r.sources().of_kind(kind))?
                .first()
                .map(|s| s.id)
                .ok_or_else(|| msg(format!("no {kind} source in settings.json")))?
        }
    };
    let src = app.store().read(|r| crate::sync::source(r, id))?;
    if src.kind != "gcal" && src.kind != "gmail" {
        return Err(msg(format!("source {id} isn't a Google source")));
    }
    Ok(src)
}

/// `gcal.auth_start` and `gcal.auth_status` (`{source_id?}` or `{kind?}`), for gcal and gmail.
pub fn call(app: &App, name: &str, args: &Value) -> Result<Value> {
    match name {
        "auth_start" => auth_start(app, args),
        "auth_status" => auth_status(app, args),
        _ => Err(crate::api::unknown(name)),
    }
}

fn auth_start(app: &App, args: &Value) -> Result<Value> {
    let src = pick_source(app, args)?;
    let prefix = prefix_of(&src.kind, &src.config);
    let names = SecretNames::of(&src.kind, &src.config);
    let missing = |n: &str| msg(format!("store your OAuth client in secret '{n}' first (see docs/sources/gcal.md)"));
    let client_id = app.platform.secret_get(&names.client_id).ok_or_else(|| missing(&names.client_id))?;
    let client_secret = app.platform.secret_get(&names.client_secret).ok_or_else(|| missing(&names.client_secret))?;

    let verifier = random_b64(48)?; // 64 chars, within RFC 7636's 43..128
    let state = random_b64(16)?;
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let redirect_uri = format!("http://127.0.0.1:{}", listener.local_addr()?.port());
    let hint = src.config.get("account").and_then(Value::as_str);
    let url = consent_url(&client_id, &redirect_uri, &scopes_of(&src.kind, &src.config), &pkce_challenge(&verifier), &state, hint);

    flows().insert(src.id, Flow::Waiting);
    let id = src.id;
    let redirect = redirect_uri.clone();
    let platform = app.platform.clone();
    let refresh_name = names.refresh_token.clone();
    std::thread::Builder::new()
        .name("gcal-consent".into())
        .spawn(move || {
            let http = UreqHttp::default();
            let outcome = serve_callback(listener, Instant::now() + CONSENT_TIMEOUT, &state, |code| {
                exchange_code(&http, &client_id, &client_secret, code, &verifier, &redirect)
            });
            // Save the refresh token right away, so closing Alethiometer can't lose it.
            let outcome = match outcome {
                Flow::Exchanged(refresh) => match platform.secret_set(&refresh_name, &refresh) {
                    Ok(()) => {
                        CACHE.forget_prefix(&prefix);
                        Flow::Connected
                    }
                    Err(e) => Flow::Failed(format!("couldn't save the refresh token: {e}")),
                },
                other => other,
            };
            flows().insert(id, outcome);
        })?;
    // Open the consent screen unless the caller only wants the URL.
    let opened = !args.get("no_open").and_then(Value::as_bool).unwrap_or(false) && app.platform.open_url(&url).is_ok();
    Ok(json!({
        "url": url,
        "redirect_uri": redirect_uri,
        "expires_in_secs": CONSENT_TIMEOUT.as_secs(),
        "opened": opened,
    }))
}

fn auth_status(app: &App, args: &Value) -> Result<Value> {
    let src = pick_source(app, args)?;
    let prefix = prefix_of(&src.kind, &src.config);
    let names = SecretNames::of(&src.kind, &src.config);
    let mut flows = flows();
    if let Some(Flow::Exchanged(refresh)) = flows.get(&src.id) {
        app.platform.secret_set(&names.refresh_token, refresh)?;
        CACHE.forget_prefix(&prefix);
        flows.insert(src.id, Flow::Connected);
    }
    let (state, error) = match flows.get(&src.id) {
        None => ("idle", None),
        Some(Flow::Waiting) => ("waiting", None),
        Some(Flow::Exchanged(_)) | Some(Flow::Connected) => ("connected", None),
        Some(Flow::Failed(e)) => ("failed", Some(e.clone())),
    };
    Ok(json!({
        "source_id": src.id,
        "state": state,
        "error": error,
        "has_client": app.platform.secret_get(&names.client_id).is_some() && app.platform.secret_get(&names.client_secret).is_some(),
        "has_refresh_token": app.platform.secret_get(&names.refresh_token).is_some(),
        "secrets": [names.client_id, names.client_secret, names.refresh_token],
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync::http::FakeHttp;

    #[test]
    fn pkce_challenge_is_s256_base64url() {
        // Computed independently: base64url(sha256(verifier)) without padding.
        assert_eq!(pkce_challenge("dBjftJeZ4CVP-mJ92K9mzvXpApYM4XqTANAJfKlLPIo"), "5AK02UWhvE-wcLJ2JUX11eybNwnNd84URykhUrBvCDM");
        let v = random_b64(48).unwrap();
        assert_eq!(v.len(), 64);
        assert!(v.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'));
    }

    #[test]
    fn gmail_shares_the_calendar_client_with_its_own_token_and_scope() {
        let names = |kind: &str, config: Value| {
            let n = SecretNames::of(kind, &config);
            [n.client_id, n.client_secret, n.refresh_token]
        };
        assert_eq!(names("gmail", json!({})), ["gcal.client_id", "gcal.client_secret", "gmail.refresh_token"]);
        assert_eq!(names("gcal", json!({})), ["gcal.client_id", "gcal.client_secret", "gcal.refresh_token"]);
        assert_eq!(names("gcal", json!({ "secret_prefix": "gcal.w2" })), ["gcal.w2.client_id", "gcal.w2.client_secret", "gcal.w2.refresh_token"]);
        assert_eq!(names("gmail", json!({ "client_prefix": "g2" }))[0], "g2.client_id");
        assert_eq!(scopes_of("gmail", &json!({})), GMAIL_SCOPE);
        assert_eq!(scopes_of("gcal", &json!({})), DEFAULT_SCOPE);
    }

    #[test]
    fn consent_url_carries_pkce_and_offline() {
        let u = consent_url("cid.apps.googleusercontent.com", "http://127.0.0.1:5555", DEFAULT_SCOPE, "chal", "st", None);
        assert!(u.starts_with("https://accounts.google.com/o/oauth2/v2/auth?client_id=cid.apps.googleusercontent.com&"));
        assert!(u.contains("redirect_uri=http%3A%2F%2F127.0.0.1%3A5555"));
        assert!(u.contains("scope=https%3A%2F%2Fwww.googleapis.com%2Fauth%2Fcalendar.events.owned"));
        assert!(u.contains("code_challenge=chal&code_challenge_method=S256&state=st&access_type=offline&prompt=consent"));
    }

    #[test]
    fn callback_parsing_checks_state() {
        let line = "GET /?state=s%2B1&code=4%2F0AbC&scope=x HTTP/1.1";
        assert_eq!(parse_callback(line, "s+1"), Callback::Code("4/0AbC".into()));
        assert_eq!(parse_callback(line, "other"), Callback::Ignore);
        assert_eq!(parse_callback("GET /favicon.ico HTTP/1.1", "s"), Callback::Ignore);
        assert_eq!(parse_callback("GET /?error=access_denied&state=s HTTP/1.1", "s"), Callback::Denied("access_denied".into()));
    }

    #[test]
    fn code_exchange_posts_verifier_and_returns_refresh_token() {
        let http = FakeHttp::default();
        http.on("POST", "oauth2.googleapis.com/token", 200, include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/gcal/token_exchange.json")));
        let r = exchange_code(&http, "cid", "csecret", "4/0AbC", "verif", "http://127.0.0.1:5555").unwrap();
        assert_eq!(r, "1//0gRefreshTokenExample");
        let body = http.requests()[0].2.clone().unwrap();
        assert_eq!(
            body,
            "client_id=cid&client_secret=csecret&code=4%2F0AbC&code_verifier=verif&grant_type=authorization_code&redirect_uri=http%3A%2F%2F127.0.0.1%3A5555"
        );
    }

    #[test]
    fn loopback_listener_exchanges_the_code() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            serve_callback(listener, Instant::now() + Duration::from_secs(10), "st8", |code| {
                assert_eq!(code, "the-code");
                Ok("refresh-1".to_string())
            })
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
