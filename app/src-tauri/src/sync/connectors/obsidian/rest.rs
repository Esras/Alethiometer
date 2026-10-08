//! Client for Obsidian's "Local REST API with MCP" community plugin
//! (coddingtonbear/obsidian-local-rest-api; checked against 5.3.0 and its
//! `docs/openapi.yaml`). Writes go through the running Obsidian app, so open
//! editors and the user's sync plugin see them as ordinary Obsidian edits.
//!
//! Only two endpoints are used:
//! - `GET /vault/{path}` with `Accept: application/vnd.olrapi.note+json`:
//!   `{ content, stat: { mtime, ctime, size }, … }`; 404 when the file is missing.
//! - `PUT /vault/{path}` with a `text/markdown` body: the plugin calls
//!   `vault.modify` (or `vault.create`), answers 204.
//!
//! The plugin has no conditional PUT (its `If-Match` works only on PATCH, and a
//! PATCH of the document root re-flows whitespace and keeps the old front
//! matter), so a write is "GET, compare, PUT" with the compare right before
//! the PUT. The window between them is a local round trip.
//!
//! TLS: the plugin serves HTTPS on 27124 with a certificate signed by its own
//! CA (name-constrained to 127.0.0.1/localhost). We trust exactly that CA and
//! nothing else, and never go through a proxy. The CA comes from the plugin's
//! settings file in the vault, or from `config.cert_path`. Plain HTTP (the
//! plugin's optional port 27123) is accepted on loopback only.

use crate::sync::http::{Http, Response, UreqHttp};
use serde_json::Value;
use std::path::{Path, PathBuf};

pub const DEFAULT_URL: &str = "https://127.0.0.1:27124";
/// Secret holding the plugin's API key (Settings → Local REST API with MCP).
pub const API_KEY: &str = "obsidian.api_key";
pub const NOT_OPEN: &str = "Obsidian isn't open (or the Local REST API plugin is off). Open Obsidian and retry (⌘R).";
/// Prefix of every conflict error, so the UI can tell a conflict from a failure.
pub const CONFLICT: &str = "conflict:";
const NOTE_JSON: &str = "application/vnd.olrapi.note+json";
const PLUGIN_ID: &str = "obsidian-local-rest-api";

#[derive(Debug)]
pub struct Note {
    pub content: String,
    pub mtime_ms: Option<i64>,
}

pub struct Rest<'a> {
    http: &'a dyn Http,
    base: String,
    auth: String,
}

impl<'a> Rest<'a> {
    pub fn new(http: &'a dyn Http, base_url: &str, api_key: &str) -> Rest<'a> {
        Rest {
            http,
            base: base_url.trim_end_matches('/').to_string(),
            auth: format!("Bearer {api_key}"),
        }
    }

    fn url(&self, path: &str) -> String {
        format!("{}/vault/{}", self.base, encode_path(path))
    }

    /// The note as Obsidian has it; None when there's no such file.
    pub fn read(&self, path: &str) -> anyhow::Result<Option<Note>> {
        let resp = self
            .http
            .request(
                "GET",
                &self.url(path),
                &[("Authorization", &self.auth), ("Accept", NOTE_JSON)],
                None,
            )
            .map_err(explain)?;
        if resp.status == 404 {
            return Ok(None);
        }
        let v = check(resp)?.json()?;
        let content = v
            .get("content")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow::anyhow!("Obsidian's answer has no content"))?;
        let mtime_ms = v.pointer("/stat/mtime").and_then(Value::as_f64).map(|m| m as i64);
        Ok(Some(Note {
            content: content.to_string(),
            mtime_ms,
        }))
    }

    /// Replace (or create) the whole file.
    pub fn write(&self, path: &str, text: &str) -> anyhow::Result<()> {
        let resp = self
            .http
            .request(
                "PUT",
                &self.url(path),
                &[
                    ("Authorization", &self.auth),
                    ("Content-Type", "text/markdown; charset=utf-8"),
                ],
                Some(text),
            )
            .map_err(explain)?;
        check(resp)?;
        Ok(())
    }
}

/// Each segment percent-encoded on its own, `/` kept: how the plugin decodes
/// vault paths (a `#` or `?` in a file name must not end the path).
fn encode_path(path: &str) -> String {
    path.split('/')
        .map(|seg| {
            seg.bytes()
                .map(|b| {
                    if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
                        (b as char).to_string()
                    } else {
                        format!("%{b:02X}")
                    }
                })
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("/")
}

fn check(resp: Response) -> anyhow::Result<Response> {
    match resp.status {
        200..=299 => Ok(resp),
        401 | 403 => anyhow::bail!(
            "Obsidian refused the API key. Copy it from Obsidian → Settings → Local REST API with MCP and store it as secret '{API_KEY}'."
        ),
        s => {
            // The plugin's errors are `{ errorCode, message }`.
            let m = resp.json().ok().and_then(|v| v.get("message").and_then(Value::as_str).map(str::to_owned));
            let m = m.unwrap_or_else(|| resp.body.chars().take(200).collect());
            anyhow::bail!("Obsidian's REST API answered HTTP {s}: {m}")
        }
    }
}

/// Turn transport errors into something the user can act on.
fn explain(e: anyhow::Error) -> anyhow::Error {
    use std::io::ErrorKind as K;
    let down = |k: K| {
        matches!(
            k,
            K::ConnectionRefused
                | K::ConnectionReset
                | K::ConnectionAborted
                | K::NotConnected
                | K::TimedOut
                | K::AddrNotAvailable
        )
    };
    for cause in e.chain() {
        if let Some(u) = cause.downcast_ref::<ureq::Error>() {
            match u {
                ureq::Error::Io(io) if down(io.kind()) => return anyhow::anyhow!(NOT_OPEN),
                ureq::Error::ConnectionFailed | ureq::Error::Timeout(_) => return anyhow::anyhow!(NOT_OPEN),
                ureq::Error::Rustls(_) | ureq::Error::Tls(_) => {
                    return anyhow::anyhow!(
                        "Obsidian's HTTPS certificate isn't signed by the Local REST API certificate Alethiometer trusts ({u}). \
                         If you regenerated it in the plugin's settings, check config.cert_path; see docs/sources/obsidian.md."
                    )
                }
                _ => {}
            }
        }
        if let Some(io) = cause.downcast_ref::<std::io::Error>() {
            if down(io.kind()) {
                return anyhow::anyhow!(NOT_OPEN);
            }
        }
    }
    e
}

// ---------------------------------------------------------------- transport

/// The plugin's base URL and an HTTP client that trusts only the plugin: a
/// pinned CA for https, loopback-only for http. Never proxied.
pub fn transport(config: &Value, vault: &Path) -> anyhow::Result<(String, Box<dyn Http>)> {
    use ureq::tls::{PemItem, RootCerts, TlsConfig};
    let url = config
        .get("rest_url")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .unwrap_or(DEFAULT_URL);
    let url = url.trim_end_matches('/').to_string();
    let (scheme, rest) = url
        .split_once("://")
        .ok_or_else(|| anyhow::anyhow!("obsidian config.rest_url must start with https:// or http://"))?;
    let host = host_of(rest);
    let builder = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .proxy(None)
        .timeout_global(Some(std::time::Duration::from_secs(10)));
    let agent = match scheme {
        "https" => {
            let (pem, from) = pinned_pem(config, vault)?;
            let certs: Vec<_> = ureq::tls::parse_pem(pem.as_bytes())
                .filter_map(|i| match i {
                    Ok(PemItem::Certificate(c)) => Some(c),
                    _ => None,
                })
                .collect();
            if certs.is_empty() {
                anyhow::bail!("no certificate found in {}", from.display());
            }
            let tls = TlsConfig::builder().root_certs(RootCerts::Specific(std::sync::Arc::new(certs))).build();
            builder.tls_config(tls).build().new_agent()
        }
        "http" if matches!(host, "127.0.0.1" | "localhost" | "[::1]") => builder.build().new_agent(),
        "http" => anyhow::bail!("obsidian config.rest_url: plain http is only allowed to 127.0.0.1 (the API key would cross the network in the clear)"),
        _ => anyhow::bail!("obsidian config.rest_url must start with https:// or http://"),
    };
    Ok((url, Box::new(UreqHttp::from_agent(agent))))
}

/// `host[:port][/…]` → host (IPv6 literals keep their brackets).
fn host_of(authority: &str) -> &str {
    let a = authority.split('/').next().unwrap_or("");
    if a.starts_with('[') {
        return a.split_inclusive(']').next().unwrap_or(a);
    }
    a.split(':').next().unwrap_or(a)
}

/// The certificate to trust, in PEM, and where it came from.
///
/// `config.cert_path` may name a PEM file (e.g. the one the plugin serves at
/// `/obsidian-local-rest-api.crt`) or the plugin's `data.json`. By default we
/// read `<vault>/.obsidian/plugins/obsidian-local-rest-api/data.json` and take
/// only `crypto.caCert` (older installs: `crypto.cert`, their self-signed
/// certificate). The file also holds the API key and private keys; those are
/// never read into anything but this parse, and never logged.
fn pinned_pem(config: &Value, vault: &Path) -> anyhow::Result<(String, PathBuf)> {
    let path = match config
        .get("cert_path")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
    {
        Some(p) => PathBuf::from(p),
        None => vault.join(".obsidian/plugins").join(PLUGIN_ID).join("data.json"),
    };
    let text = std::fs::read_to_string(&path).map_err(|_| {
        anyhow::anyhow!(
            "Can't read the Local REST API certificate to trust at {}. Install and enable the plugin in this vault, \
             or set config.cert_path to the certificate from {DEFAULT_URL}/{PLUGIN_ID}.crt",
            path.display()
        )
    })?;
    if text.trim_start().starts_with('{') {
        let v: Value =
            serde_json::from_str(&text).map_err(|_| anyhow::anyhow!("{} isn't valid JSON", path.display()))?;
        let pem = v
            .pointer("/crypto/caCert")
            .or_else(|| v.pointer("/crypto/cert"))
            .and_then(Value::as_str);
        let pem = pem.ok_or_else(|| {
            anyhow::anyhow!(
                "no certificate in {} yet; open Obsidian once with the plugin enabled",
                path.display()
            )
        })?;
        return Ok((pem.to_string(), path));
    }
    Ok((text, path))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync::http::FakeHttp;
    use serde_json::json;

    const NOTE: &str = include_str!("../../../../tests/fixtures/obsidian/note.json");

    #[test]
    fn read_parses_note_json_and_sends_the_key() {
        let http = FakeHttp::default();
        http.on("GET", "/vault/Topics/Datadog%20renewal.md", 200, NOTE);
        let api = Rest::new(&http, "https://127.0.0.1:27124/", "k3y");
        let n = api.read("Topics/Datadog renewal.md").unwrap().unwrap();
        assert!(n.content.starts_with("## Where we are\n"));
        assert_eq!(n.mtime_ms, Some(1790592723000));
        let reqs = http.requests();
        assert_eq!(reqs[0].1, "https://127.0.0.1:27124/vault/Topics/Datadog%20renewal.md");
        let h = &http.header_log.lock().unwrap()[0];
        assert!(h.contains(&("Authorization".into(), "Bearer k3y".into())));
        assert!(h.contains(&("Accept".into(), NOTE_JSON.into())));
    }

    #[test]
    fn missing_files_and_errors() {
        let http = FakeHttp::default();
        http.on("GET", "/vault/", 404, r#"{"errorCode":40400,"message":"Not Found"}"#);
        let api = Rest::new(&http, DEFAULT_URL, "k");
        assert!(api.read("nope.md").unwrap().is_none());

        http.on(
            "GET",
            "/vault/",
            401,
            r#"{"errorCode":40101,"message":"Authorization required."}"#,
        );
        let e = api.read("a.md").unwrap_err().to_string();
        assert!(
            e.contains("refused the API key") && e.contains("obsidian.api_key"),
            "{e}"
        );
        assert!(!e.contains("Bearer"), "{e}");

        http.on(
            "PUT",
            "/vault/",
            405,
            r#"{"errorCode":40510,"message":"Request method is valid only for file paths, not directories."}"#,
        );
        let e = api.write("dir/", "x").unwrap_err().to_string();
        assert!(
            e.contains("HTTP 405: Request method is valid only for file paths"),
            "{e}"
        );
    }

    #[test]
    fn write_puts_markdown() {
        let http = FakeHttp::default();
        http.on("PUT", "/vault/a%23b/n%3F.md", 204, "");
        Rest::new(&http, DEFAULT_URL, "k")
            .write("a#b/n?.md", "hello\n")
            .unwrap();
        assert_eq!(http.requests()[0].2.as_deref(), Some("hello\n"));
        let h = &http.header_log.lock().unwrap()[0];
        assert!(h.contains(&("Content-Type".into(), "text/markdown; charset=utf-8".into())));
    }

    #[test]
    fn connection_refused_means_obsidian_is_closed() {
        // A port that was just free: nothing listens there.
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let vault = tempfile::tempdir().unwrap();
        let (url, http) = transport(&json!({ "rest_url": format!("http://127.0.0.1:{port}") }), vault.path()).unwrap();
        let e = Rest::new(http.as_ref(), &url, "k")
            .read("a.md")
            .unwrap_err()
            .to_string();
        assert_eq!(e, NOT_OPEN);
    }

    #[test]
    fn transport_rules() {
        let vault = tempfile::tempdir().unwrap();
        // https without the plugin installed: says where it looked.
        let e = transport(&json!({}), vault.path()).err().unwrap().to_string();
        assert!(e.contains("obsidian-local-rest-api/data.json"), "{e}");
        // Plain http only on loopback.
        assert!(transport(&json!({ "rest_url": "http://192.168.1.5:27123" }), vault.path()).is_err());
        assert!(transport(&json!({ "rest_url": "http://localhost:27123" }), vault.path()).is_ok());
        assert!(transport(&json!({ "rest_url": "ftp://x" }), vault.path()).is_err());
        // The CA is read from the plugin's data.json (only the certificate).
        let dir = vault.path().join(".obsidian/plugins/obsidian-local-rest-api");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("data.json"),
            json!({ "apiKey": "secret", "crypto": { "cert": "x" } }).to_string(),
        )
        .unwrap();
        let e = transport(&json!({}), vault.path()).err().unwrap().to_string();
        assert!(e.contains("no certificate found"), "{e}");
        assert!(!e.contains("secret"));
        assert_eq!(host_of("[::1]:27123/x"), "[::1]");
        assert_eq!(host_of("127.0.0.1:27124"), "127.0.0.1");
    }

    /// Real TLS against a server shaped like the plugin's (CA with name
    /// constraints, leaf with an IP SAN): trusted when pinned, refused with any
    /// other CA. Needs the `openssl` CLI; skipped without it.
    #[test]
    fn pinned_ca_is_the_only_trust() {
        let Some(pki) = TestPki::new() else {
            eprintln!("pinned_ca_is_the_only_trust: skipped (no openssl CLI)");
            return;
        };
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let mut server = std::process::Command::new("openssl")
            .args(["s_server", "-quiet", "-www", "-accept"])
            .arg(format!("127.0.0.1:{port}"))
            .arg("-cert")
            .arg(pki.dir.path().join("leaf.pem"))
            .arg("-key")
            .arg(pki.dir.path().join("leaf.key"))
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();
        let until = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while std::net::TcpStream::connect(("127.0.0.1", port)).is_err() && std::time::Instant::now() < until {
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        let vault = tempfile::tempdir().unwrap();
        let url = format!("https://127.0.0.1:{port}");
        let get = |cert: &Path| {
            let (url, http) = transport(&json!({ "rest_url": url, "cert_path": cert }), vault.path())?;
            http.get(&format!("{url}/"), &[]).map_err(explain)
        };
        let ok = get(&pki.dir.path().join("ca.pem"));
        let bad = get(&pki.dir.path().join("other.pem"));
        let _ = server.kill();
        let _ = server.wait();
        assert_eq!(ok.unwrap().status, 200);
        let e = bad.unwrap_err().to_string();
        assert!(e.contains("certificate"), "{e}");
    }

    struct TestPki {
        dir: tempfile::TempDir,
    }

    impl TestPki {
        fn new() -> Option<TestPki> {
            let dir = tempfile::tempdir().ok()?;
            let d = dir.path();
            let run = |args: &[&str]| {
                std::process::Command::new("openssl")
                    .current_dir(d)
                    .args(args)
                    .output()
                    .ok()
                    .filter(|o| o.status.success())
                    .map(|_| ())
            };
            // The plugin's shape (src/certificates.ts): a name-constrained CA, and a
            // leaf for 127.0.0.1 with serverAuth.
            std::fs::write(
                d.join("leaf.cnf"),
                "[leaf]\nbasicConstraints=critical,CA:FALSE\nkeyUsage=critical,digitalSignature,keyEncipherment\n\
                 extendedKeyUsage=serverAuth\nsubjectAltName=IP:127.0.0.1\n",
            )
            .ok()?;
            for ca in ["ca", "other"] {
                run(&[
                    "req",
                    "-x509",
                    "-newkey",
                    "rsa:2048",
                    "-nodes",
                    "-keyout",
                    &format!("{ca}.key"),
                    "-out",
                    &format!("{ca}.pem"),
                    "-days",
                    "2",
                    "-subj",
                    "/CN=Obsidian Local REST API CA",
                    "-addext",
                    "basicConstraints=critical,CA:TRUE",
                    "-addext",
                    "keyUsage=critical,keyCertSign,cRLSign",
                    "-addext",
                    "nameConstraints=critical,permitted;IP:127.0.0.1/255.255.255.255,permitted;DNS:localhost",
                ])?;
            }
            run(&[
                "req",
                "-newkey",
                "rsa:2048",
                "-nodes",
                "-keyout",
                "leaf.key",
                "-out",
                "leaf.csr",
                "-subj",
                "/CN=Obsidian Local REST API",
            ])?;
            run(&[
                "x509",
                "-req",
                "-in",
                "leaf.csr",
                "-CA",
                "ca.pem",
                "-CAkey",
                "ca.key",
                "-CAcreateserial",
                "-out",
                "leaf.pem",
                "-days",
                "2",
                "-extfile",
                "leaf.cnf",
                "-extensions",
                "leaf",
            ])?;
            Some(TestPki { dir })
        }
    }
}
