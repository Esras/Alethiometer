//! Google OAuth 2.0 for a desktop ("installed") app: PKCE plus a loopback
//! redirect on 127.0.0.1. The user brings their own OAuth client (Desktop type).
//! What sign-ins share (listener, PKCE, token cache) is `connectors::oauth`;
//! `gcal.auth_start` / `gcal.auth_status` are this provider's two calls.

pub use super::super::oauth::{cache_key, form, pkce_challenge, random_b64, serve_callback, TokenCache, CACHE};
use super::super::oauth::{flows, Flow, CONSENT_TIMEOUT, EXPIRY_MARGIN};
use crate::error::{msg, Result};
use crate::state::App;
use crate::sync::http::{Http, UreqHttp};
use crate::sync::Ctx;
use serde_json::{json, Value};
use std::net::TcpListener;
use std::time::{Duration, Instant};

pub const AUTH_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";
pub const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
/// Read, create and change events on calendars the user owns (their primary
/// calendar included). Enough for polling, RSVP and holds; nothing else.
pub const DEFAULT_SCOPE: &str = "https://www.googleapis.com/auth/calendar.events.owned";
/// Read mail and change labels (mark read); no delete, no send.
pub const GMAIL_SCOPE: &str = "https://www.googleapis.com/auth/gmail.modify";

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
        let client = config
            .get("client_prefix")
            .and_then(Value::as_str)
            .unwrap_or(default_client);
        SecretNames {
            client_id: format!("{client}.client_id"),
            client_secret: format!("{client}.client_secret"),
            refresh_token: format!("{prefix}.refresh_token"),
        }
    }
}

pub fn prefix_of(kind: &str, config: &Value) -> String {
    config
        .get("secret_prefix")
        .and_then(Value::as_str)
        .unwrap_or(kind)
        .to_string()
}

pub fn scopes_of(kind: &str, config: &Value) -> String {
    match config.get("scopes").and_then(Value::as_array) {
        Some(list) if !list.is_empty() => list.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(" "),
        _ if kind == "gmail" => GMAIL_SCOPE.to_string(),
        _ => DEFAULT_SCOPE.to_string(),
    }
}

// ---------------------------------------------------------------- access tokens

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

fn refresh_access_token(
    http: &dyn Http,
    client_id: &str,
    client_secret: &str,
    refresh: &str,
) -> anyhow::Result<(String, u64)> {
    let body = form(&[
        ("client_id", client_id),
        ("client_secret", client_secret),
        ("grant_type", "refresh_token"),
        ("refresh_token", refresh),
    ]);
    let resp = http.request(
        "POST",
        TOKEN_URL,
        &[("content-type", "application/x-www-form-urlencoded")],
        Some(&body),
    )?;
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
    let token = v
        .get("access_token")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("token response had no access_token"))?;
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
    let resp = http.request(
        "POST",
        TOKEN_URL,
        &[("content-type", "application/x-www-form-urlencoded")],
        Some(&body),
    )?;
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

pub fn consent_url(
    client_id: &str,
    redirect_uri: &str,
    scope: &str,
    challenge: &str,
    state: &str,
    login_hint: Option<&str>,
) -> String {
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

// ---------------------------------------------------------------- consent API

/// `source_id`, else the first source of `kind` (default `gcal`).
fn pick_source(app: &App, args: &Value) -> Result<crate::sync::Source> {
    let id = match crate::api::opt_i64(args, "source_id") {
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
    let missing = |n: &str| {
        msg(format!(
            "store your OAuth client in secret '{n}' first (see docs/sources/gcal.md)"
        ))
    };
    let client_id = app
        .platform
        .secret_get(&names.client_id)
        .ok_or_else(|| missing(&names.client_id))?;
    let client_secret = app
        .platform
        .secret_get(&names.client_secret)
        .ok_or_else(|| missing(&names.client_secret))?;

    let verifier = random_b64(48)?; // 64 chars, within RFC 7636's 43..128
    let state = random_b64(16)?;
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let redirect_uri = format!("http://127.0.0.1:{}", listener.local_addr()?.port());
    let hint = src.config.get("account").and_then(Value::as_str);
    let url = consent_url(
        &client_id,
        &redirect_uri,
        &scopes_of(&src.kind, &src.config),
        &pkce_challenge(&verifier),
        &state,
        hint,
    );

    flows().insert(src.id, Flow::Waiting);
    let id = src.id;
    let redirect = redirect_uri.clone();
    let platform = app.platform.clone();
    let refresh_name = names.refresh_token.clone();
    std::thread::Builder::new().name("gcal-consent".into()).spawn(move || {
        let http = UreqHttp::default();
        let outcome = serve_callback(listener, Instant::now() + CONSENT_TIMEOUT, &state, "Google", |code| {
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
    fn gmail_shares_the_calendar_client_with_its_own_token_and_scope() {
        let names = |kind: &str, config: Value| {
            let n = SecretNames::of(kind, &config);
            [n.client_id, n.client_secret, n.refresh_token]
        };
        assert_eq!(
            names("gmail", json!({})),
            ["gcal.client_id", "gcal.client_secret", "gmail.refresh_token"]
        );
        assert_eq!(
            names("gcal", json!({})),
            ["gcal.client_id", "gcal.client_secret", "gcal.refresh_token"]
        );
        assert_eq!(
            names("gcal", json!({ "secret_prefix": "gcal.w2" })),
            ["gcal.w2.client_id", "gcal.w2.client_secret", "gcal.w2.refresh_token"]
        );
        assert_eq!(names("gmail", json!({ "client_prefix": "g2" }))[0], "g2.client_id");
        assert_eq!(scopes_of("gmail", &json!({})), GMAIL_SCOPE);
        assert_eq!(scopes_of("gcal", &json!({})), DEFAULT_SCOPE);
    }

    #[test]
    fn consent_url_carries_pkce_and_offline() {
        let u = consent_url(
            "cid.apps.googleusercontent.com",
            "http://127.0.0.1:5555",
            DEFAULT_SCOPE,
            "chal",
            "st",
            None,
        );
        assert!(u.starts_with("https://accounts.google.com/o/oauth2/v2/auth?client_id=cid.apps.googleusercontent.com&"));
        assert!(u.contains("redirect_uri=http%3A%2F%2F127.0.0.1%3A5555"));
        assert!(u.contains("scope=https%3A%2F%2Fwww.googleapis.com%2Fauth%2Fcalendar.events.owned"));
        assert!(
            u.contains("code_challenge=chal&code_challenge_method=S256&state=st&access_type=offline&prompt=consent")
        );
    }

    #[test]
    fn code_exchange_posts_verifier_and_returns_refresh_token() {
        let http = FakeHttp::default();
        http.on(
            "POST",
            "oauth2.googleapis.com/token",
            200,
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/gcal/token_exchange.json"
            )),
        );
        let r = exchange_code(&http, "cid", "csecret", "4/0AbC", "verif", "http://127.0.0.1:5555").unwrap();
        assert_eq!(r, "1//0gRefreshTokenExample");
        let body = http.requests()[0].2.clone().unwrap();
        assert_eq!(
            body,
            "client_id=cid&client_secret=csecret&code=4%2F0AbC&code_verifier=verif&grant_type=authorization_code&redirect_uri=http%3A%2F%2F127.0.0.1%3A5555"
        );
    }
}
