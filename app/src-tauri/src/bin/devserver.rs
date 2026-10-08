//! Browser-mode bridge for UI development without WebKitGTK:
//! `POST http://127.0.0.1:1421/call/<cmd>` with a JSON body of args and the
//! header `X-Alethiometer: 1`. Vite proxies `/call` here, so the UI calls it
//! from its own origin. Uses `ALETHIOMETER_DATA_DIR` (default `./.alethiometer-dev`).
//!
//! Any web page open in the same browser can send requests to localhost, so
//! the server sends no CORS headers and refuses requests that a page on
//! another site could make:
//! - without `X-Alethiometer`: a cross-site page can't add a custom header
//!   without a CORS preflight, which gets no approval here;
//! - with a `Host` other than this machine's loopback names: a page on a
//!   DNS-rebound domain is same-origin to itself, but its `Host` is its domain;
//! - with an `Origin` that isn't a loopback page (the Vite UI).

use alethiometer_lib::state::App;
use tiny_http::{Header, Method, Request, Response, Server};

const HEADER: &str = "x-alethiometer";

fn main() -> anyhow::Result<()> {
    let dir: std::path::PathBuf = std::env::var_os("ALETHIOMETER_DATA_DIR")
        .map(Into::into)
        .unwrap_or_else(|| ".alethiometer-dev".into());
    let app = App::shared(App::open(&dir)?);
    alethiometer_lib::sync::start(app.clone());
    alethiometer_lib::settings::live::watch(app.clone());
    alethiometer_lib::embed::start(app.clone());
    alethiometer_lib::queries::notes::warm(app.clone());
    alethiometer_lib::outbox::start(app.clone());
    alethiometer_lib::agents::start(app.clone());
    let addr = std::env::var("ALETHIOMETER_DEVSERVER_ADDR").unwrap_or_else(|_| "127.0.0.1:1421".into());
    let server = Server::http(&addr).map_err(|e| anyhow::anyhow!("{e}"))?;
    eprintln!(
        "alethiometer devserver on http://{addr} (data: {})",
        app.data_dir.display()
    );
    let json_h = Header::from_bytes("Content-Type", "application/json").unwrap();
    for mut req in server.incoming_requests() {
        if let Err(why) = allowed(&req) {
            let _ = req.respond(Response::from_string(why).with_status_code(403));
            continue;
        }
        // Each call on its own thread, as in the desktop app: `live.wait`
        // holds its request open for up to 25 s.
        let (app, json_h) = (app.clone(), json_h.clone());
        std::thread::spawn(move || {
            let cmd = req.url().trim_start_matches("/call/").to_string();
            let mut body = String::new();
            let _ = req.as_reader().read_to_string(&mut body);
            let args: serde_json::Value = serde_json::from_str(&body).unwrap_or(serde_json::Value::Null);
            let (status, out) = match alethiometer_lib::api::call(&app, &cmd, &args) {
                Ok(v) => (200, v),
                Err(e) => (400, serde_json::json!({ "error": e.to_string() })),
            };
            let _ = req.respond(
                Response::from_string(out.to_string())
                    .with_status_code(status)
                    .with_header(json_h),
            );
        });
    }
    Ok(())
}

fn header<'a>(req: &'a Request, name: &'static str) -> Option<&'a str> {
    req.headers()
        .iter()
        .find(|h| h.field.equiv(name))
        .map(|h| h.value.as_str())
}

fn allowed(req: &Request) -> Result<(), &'static str> {
    check(
        req.method(),
        req.url(),
        header(req, "host"),
        header(req, "origin"),
        header(req, HEADER),
    )
}

/// A loopback host name, with or without a port.
fn loopback(host: &str) -> bool {
    let name = match host.strip_prefix('[') {
        Some(v6) => v6.split(']').next().unwrap_or(""),
        None => host.rsplit_once(':').map_or(
            host,
            |(h, p)| if p.chars().all(|c| c.is_ascii_digit()) { h } else { host },
        ),
    };
    matches!(name, "127.0.0.1" | "localhost" | "::1")
}

fn check(
    method: &Method,
    url: &str,
    host: Option<&str>,
    origin: Option<&str>,
    marker: Option<&str>,
) -> Result<(), &'static str> {
    if method != &Method::Post || !url.starts_with("/call/") {
        return Err("POST /call/<cmd> only");
    }
    if !host.is_some_and(loopback) {
        return Err("Host must be this machine");
    }
    if let Some(o) = origin {
        let o = o.strip_prefix("http://").unwrap_or("");
        if !loopback(o) {
            return Err("calls from other sites are refused");
        }
    }
    if marker != Some("1") {
        return Err("missing X-Alethiometer header");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok(host: Option<&str>, origin: Option<&str>, marker: Option<&str>) -> bool {
        check(&Method::Post, "/call/app.info", host, origin, marker).is_ok()
    }

    #[test]
    fn the_ui_and_local_tools_get_through() {
        assert!(
            ok(Some("127.0.0.1:1421"), Some("http://localhost:1420"), Some("1")),
            "the Vite UI through its proxy"
        );
        assert!(ok(Some("127.0.0.1:1421"), None, Some("1")), "curl and tests");
        assert!(ok(Some("localhost:1421"), Some("http://127.0.0.1:1420"), Some("1")));
        assert!(ok(Some("[::1]:1421"), None, Some("1")));
    }

    #[test]
    fn other_sites_are_refused() {
        assert!(
            !ok(Some("127.0.0.1:1421"), None, None),
            "a plain cross-site form or text/plain POST has no marker"
        );
        assert!(
            !ok(Some("127.0.0.1:1421"), Some("https://evil.example"), Some("1")),
            "another origin"
        );
        assert!(!ok(
            Some("127.0.0.1:1421"),
            Some("http://localhost.evil.example"),
            Some("1")
        ));
        assert!(
            !ok(
                Some("rebound.evil.example:1421"),
                Some("http://rebound.evil.example:1421"),
                Some("1")
            ),
            "DNS rebinding"
        );
        assert!(!ok(None, None, Some("1")));
        assert!(check(&Method::Get, "/call/app.info", Some("127.0.0.1:1421"), None, Some("1")).is_err());
        assert!(
            check(
                &Method::Options,
                "/call/app.info",
                Some("127.0.0.1:1421"),
                None,
                Some("1")
            )
            .is_err(),
            "no preflight approval"
        );
    }
}
