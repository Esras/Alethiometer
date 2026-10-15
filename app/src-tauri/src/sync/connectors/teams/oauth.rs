//! Microsoft sign-in for Teams: OAuth 2.0 authorization code with PKCE, as a
//! public client (no client secret), redirect to `http://localhost:<port>`
//! (Entra ignores the port only for `localhost`; the listener is on
//! 127.0.0.1). The user registers the app in their tenant
//! (`docs/sources/teams.md`). What sign-ins share is `connectors::oauth`;
//! `teams.auth_start` / `teams.auth_status` are this provider's two calls.
//!
//! Microsoft rotates refresh tokens: each refresh returns a new one, which is
//! stored (`Ctx::save_secret`). The old one isn't revoked, so a failed save
//! only means signing in again within 90 days.
//!
//! Scopes are staged: chats need only permissions a user can usually grant
//! (`CHAT_SCOPES`); channels add `ChannelMessage.Read.All`, which needs an
//! admin, and are asked for only when the source lists channels.

use super::super::oauth::{
    cache_key, flows, form, pkce_challenge, random_b64, serve_callback, Flow, CACHE, CONSENT_TIMEOUT, EXPIRY_MARGIN,
};
use super::Config;
use crate::error::{msg, Result};
use crate::state::App;
use crate::sync::http::{Http, UreqHttp};
use crate::sync::Ctx;
use serde_json::{json, Value};
use std::net::TcpListener;
use std::time::{Duration, Instant};

const LOGIN: &str = "https://login.microsoftonline.com";
const GRAPH: &str = "https://graph.microsoft.com/";
/// Your profile and tenant; others' names and emails (People notes); read,
/// mark read and react in your chats; send chat messages.
const CHAT_SCOPES: &[&str] = &["User.Read", "User.ReadBasic.All", "Chat.ReadWrite", "ChatMessage.Send"];
/// Read and post in the channels listed in settings (ChannelMessage.Read.All needs an admin).
const CHANNEL_SCOPES: &[&str] = &[
    "Team.ReadBasic.All",
    "Channel.ReadBasic.All",
    "ChannelMessage.Read.All",
    "ChannelMessage.Send",
];

pub fn refresh_secret(cfg: &Config) -> String {
    format!("{}.refresh_token", cfg.secret_prefix)
}

/// Space-separated, Graph's as full URIs. `offline_access` for a refresh token.
pub fn scopes(cfg: &Config) -> String {
    let mut s = vec!["offline_access".to_string()];
    let channels = if cfg.channels.is_empty() {
        &[][..]
    } else {
        CHANNEL_SCOPES
    };
    s.extend(CHAT_SCOPES.iter().chain(channels).map(|p| format!("{GRAPH}{p}")));
    s.join(" ")
}

fn token_url(tenant: &str) -> String {
    format!("{LOGIN}/{}/oauth2/v2.0/token", super::api::enc(tenant))
}

pub fn consent_url(cfg: &Config, redirect_uri: &str, challenge: &str, state: &str) -> String {
    let params = [
        ("client_id", cfg.client_id.as_str()),
        ("response_type", "code"),
        ("redirect_uri", redirect_uri),
        ("response_mode", "query"),
        ("scope", &scopes(cfg)),
        ("state", state),
        ("code_challenge", challenge),
        ("code_challenge_method", "S256"),
    ];
    format!(
        "{LOGIN}/{}/oauth2/v2.0/authorize?{}",
        super::api::enc(&cfg.tenant),
        form(&params)
    )
}

/// A token request; `(access token, seconds to live, refresh token)`.
fn token_request(http: &dyn Http, cfg: &Config, pairs: &[(&str, &str)]) -> anyhow::Result<(String, u64, String)> {
    let resp = http.request(
        "POST",
        &token_url(&cfg.tenant),
        &[("content-type", "application/x-www-form-urlencoded")],
        Some(&form(pairs)),
    )?;
    let v: Value = resp.json().unwrap_or(Value::Null);
    if !(200..300).contains(&resp.status) {
        // `{error, error_description}`; the description starts with an AADSTS code and never echoes secrets.
        let err = v.get("error").and_then(Value::as_str).unwrap_or("unknown_error");
        let desc: String = v
            .get("error_description")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .lines()
            .next()
            .unwrap_or_default()
            .chars()
            .take(240)
            .collect();
        if err == "invalid_grant" {
            anyhow::bail!(
                "Microsoft refused the sign-in (expired, revoked or changed permissions): press Connect. {desc}"
            );
        }
        anyhow::bail!("Microsoft token request failed: HTTP {} {err} {desc}", resp.status);
    }
    let s = |k: &str| v.get(k).and_then(Value::as_str).unwrap_or_default().to_owned();
    let access = s("access_token");
    if access.is_empty() {
        anyhow::bail!("Microsoft's token response had no access_token");
    }
    Ok((
        access,
        v.get("expires_in").and_then(Value::as_u64).unwrap_or(3599),
        s("refresh_token"),
    ))
}

/// Exchange the redirect's code for a refresh token.
pub fn exchange_code(
    http: &dyn Http,
    cfg: &Config,
    code: &str,
    verifier: &str,
    redirect_uri: &str,
) -> anyhow::Result<String> {
    let scope = scopes(cfg);
    let (_, _, refresh) = token_request(
        http,
        cfg,
        &[
            ("client_id", &cfg.client_id),
            ("grant_type", "authorization_code"),
            ("code", code),
            ("redirect_uri", redirect_uri),
            ("code_verifier", verifier),
            ("scope", &scope),
        ],
    )?;
    if refresh.is_empty() {
        anyhow::bail!("Microsoft returned no refresh token (offline_access not granted?)");
    }
    Ok(refresh)
}

/// A valid access token for this source, refreshing (and storing the
/// rotated refresh token) when needed.
pub fn access_token(ctx: &Ctx, cfg: &Config) -> anyhow::Result<String> {
    let name = refresh_secret(cfg);
    let refresh = (ctx.secret)(&name).ok_or_else(|| {
        anyhow::anyhow!(
            "'{}' isn't connected to Microsoft yet (no secret '{name}'): press Connect",
            ctx.source.name
        )
    })?;
    if let Some(t) = CACHE.get(&cache_key(&cfg.secret_prefix, &refresh)) {
        return Ok(t);
    }
    if cfg.client_id.is_empty() {
        anyhow::bail!("'{}' needs config.client_id (the app registration's)", ctx.source.name);
    }
    let scope = scopes(cfg);
    let (token, expires_in, rotated) = token_request(
        ctx.http,
        cfg,
        &[
            ("client_id", &cfg.client_id),
            ("grant_type", "refresh_token"),
            ("refresh_token", &refresh),
            ("scope", &scope),
        ],
    )?;
    let life = Duration::from_secs(expires_in).saturating_sub(EXPIRY_MARGIN);
    let mut current = refresh;
    if !rotated.is_empty() && rotated != current {
        match (ctx.save_secret)(&name, &rotated) {
            Ok(()) => current = rotated,
            // The old token still works until it expires.
            Err(e) => eprintln!("teams: couldn't store the new refresh token: {e}"),
        }
    }
    CACHE.put(&cache_key(&cfg.secret_prefix, &current), &token, Instant::now() + life);
    Ok(token)
}

/// Drop the cached token after a 401 so the next call refreshes.
pub fn invalidate(ctx: &Ctx, cfg: &Config) {
    if let Some(refresh) = (ctx.secret)(&refresh_secret(cfg)) {
        CACHE.forget(&cache_key(&cfg.secret_prefix, &refresh));
    }
}

// ---------------------------------------------------------------- consent API

fn pick_source(app: &App, args: &Value) -> Result<crate::sync::Source> {
    let id = match crate::api::opt_i64(args, "source_id") {
        Some(id) => id,
        None => app
            .store()
            .read(|r| r.sources().of_kind("teams"))?
            .first()
            .map(|s| s.id)
            .ok_or_else(|| msg("no teams source in settings.json"))?,
    };
    let src = app.store().read(|r| crate::sync::source(r, id))?;
    if src.kind != "teams" {
        return Err(msg(format!("source {id} isn't a Teams source")));
    }
    Ok(src)
}

/// `teams.auth_start` and `teams.auth_status` (`{source_id?}`).
pub fn call(app: &App, name: &str, args: &Value) -> Result<Value> {
    match name {
        "auth_start" => auth_start(app, args),
        "auth_status" => auth_status(app, args),
        _ => Err(crate::api::unknown(name)),
    }
}

fn auth_start(app: &App, args: &Value) -> Result<Value> {
    let src = pick_source(app, args)?;
    let cfg = Config::from_json(&src.config);
    if cfg.client_id.is_empty() {
        return Err(msg(
            "set the app registration's client ID first (Settings › Sources › Teams; see docs/sources/teams.md)",
        ));
    }
    let verifier = random_b64(48)?;
    let state = random_b64(16)?;
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let redirect_uri = format!("http://localhost:{}", listener.local_addr()?.port());
    let url = consent_url(&cfg, &redirect_uri, &pkce_challenge(&verifier), &state);

    flows().insert(src.id, Flow::Waiting);
    let id = src.id;
    let redirect = redirect_uri.clone();
    let platform = app.platform.clone();
    std::thread::Builder::new()
        .name("teams-consent".into())
        .spawn(move || {
            let http = UreqHttp::default();
            let outcome = serve_callback(
                listener,
                Instant::now() + CONSENT_TIMEOUT,
                &state,
                "Microsoft",
                |code| exchange_code(&http, &cfg, code, &verifier, &redirect),
            );
            // Save the refresh token right away, so closing Alethiometer can't lose it.
            let outcome = match outcome {
                Flow::Exchanged(refresh) => match platform.secret_set(&refresh_secret(&cfg), &refresh) {
                    Ok(()) => {
                        CACHE.forget_prefix(&cfg.secret_prefix);
                        Flow::Connected
                    }
                    Err(e) => Flow::Failed(format!("couldn't save the refresh token: {e}")),
                },
                other => other,
            };
            flows().insert(id, outcome);
        })?;
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
    let cfg = Config::from_json(&src.config);
    let name = refresh_secret(&cfg);
    let mut flows = flows();
    if let Some(Flow::Exchanged(refresh)) = flows.get(&src.id) {
        app.platform.secret_set(&name, refresh)?;
        CACHE.forget_prefix(&cfg.secret_prefix);
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
        "has_client": !cfg.client_id.is_empty(),
        "has_refresh_token": app.platform.secret_get(&name).is_some(),
        "secrets": [name],
        "scopes": scopes(&cfg),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync::http::FakeHttp;

    fn cfg(channels: &[&str]) -> Config {
        Config::from_json(&json!({
            "client_id": "cid",
            "tenant": "contoso.onmicrosoft.com",
            "channels": channels,
        }))
    }

    #[test]
    fn channel_permissions_are_asked_only_with_channels() {
        let chats = scopes(&cfg(&[]));
        assert!(chats.starts_with("offline_access https://graph.microsoft.com/User.Read "));
        assert!(chats.contains("/Chat.ReadWrite") && chats.contains("/ChatMessage.Send"));
        assert!(!chats.contains("Channel"), "{chats}");
        let both = scopes(&cfg(&["t1|19:c@thread.tacv2"]));
        assert!(both.contains("/ChannelMessage.Read.All") && both.contains("/ChannelMessage.Send"));
    }

    #[test]
    fn consent_url_is_the_tenants_with_pkce() {
        let u = consent_url(&cfg(&[]), "http://localhost:5555", "chal", "st");
        assert!(u.starts_with(
            "https://login.microsoftonline.com/contoso.onmicrosoft.com/oauth2/v2.0/authorize?client_id=cid&response_type=code&redirect_uri=http%3A%2F%2Flocalhost%3A5555&response_mode=query&scope=offline_access%20https%3A%2F%2Fgraph.microsoft.com%2FUser.Read"
        ), "{u}");
        assert!(u.ends_with("&state=st&code_challenge=chal&code_challenge_method=S256"));
    }

    #[test]
    fn a_public_client_exchanges_the_code_without_a_secret() {
        let http = FakeHttp::default();
        http.on(
            "POST",
            "contoso.onmicrosoft.com/oauth2/v2.0/token",
            200,
            include_str!("../../../../tests/fixtures/teams/token.json"),
        );
        let r = exchange_code(&http, &cfg(&[]), "code-1", "verif", "http://localhost:5555").unwrap();
        assert_eq!(r, "0.AQ-refresh-2");
        let body = http.requests()[0].2.clone().unwrap();
        assert!(body.starts_with(
            "client_id=cid&grant_type=authorization_code&code=code-1&redirect_uri=http%3A%2F%2Flocalhost%3A5555&code_verifier=verif&scope=offline_access"
        ), "{body}");
        assert!(!body.contains("client_secret"));
    }

    #[test]
    fn a_rotated_refresh_token_is_stored_and_the_access_token_reused() {
        let http = FakeHttp::default();
        http.on(
            "POST",
            "/oauth2/v2.0/token",
            200,
            include_str!("../../../../tests/fixtures/teams/token.json"),
        );
        let stored = std::sync::Mutex::new(String::from("0.AQ-refresh-1"));
        let secret = |_: &str| Some(stored.lock().unwrap().clone());
        let saved = std::sync::Mutex::new(vec![]);
        let save = |k: &str, v: &str| {
            saved.lock().unwrap().push((k.to_owned(), v.to_owned()));
            *stored.lock().unwrap() = v.to_owned();
            Ok(())
        };
        let source = crate::sync::Source {
            id: 1,
            kind: "teams".into(),
            tag: "work".into(),
            name: "Teams".into(),
            config: json!({}),
            cursor: None,
        };
        let ctx = Ctx {
            source: &source,
            http: &http,
            secret: &secret,
            save_secret: &save,
            now: crate::time::Ts::from_unix_secs(1_800_000_000.0).unwrap(),
            clock: crate::clock::Clock::new(""),
        };
        let mut c = cfg(&[]);
        c.secret_prefix = "teams.rotation-test".into();
        assert_eq!(access_token(&ctx, &c).unwrap(), "eyJ-access");
        assert_eq!(
            *saved.lock().unwrap(),
            [(
                "teams.rotation-test.refresh_token".to_string(),
                "0.AQ-refresh-2".to_string()
            )]
        );
        // The next call finds the token under the stored (rotated) refresh token.
        assert_eq!(access_token(&ctx, &c).unwrap(), "eyJ-access");
        assert_eq!(http.requests().len(), 1);
        let body = http.requests()[0].2.clone().unwrap();
        assert!(
            body.contains("grant_type=refresh_token&refresh_token=0.AQ-refresh-1&scope="),
            "{body}"
        );
    }

    #[test]
    fn an_expired_sign_in_says_to_connect_again() {
        let http = FakeHttp::default();
        http.on(
            "POST",
            "/oauth2/v2.0/token",
            400,
            r#"{"error":"invalid_grant","error_description":"AADSTS700082: The refresh token has expired due to inactivity.\r\nTrace ID: x"}"#,
        );
        let e = token_request(&http, &cfg(&[]), &[]).unwrap_err().to_string();
        assert!(e.contains("press Connect") && e.contains("AADSTS700082"), "{e}");
        assert!(!e.contains("Trace ID"));
    }
}
