//! Minimal blocking HTTP behind a trait, so connectors can be tested with
//! canned responses (`FakeHttp`).

use serde_json::Value;

#[derive(Debug, Clone)]
pub struct Response {
    pub status: u16,
    pub body: String,
    /// Response headers, names lowercased (e.g. `retry-after` on a 429).
    pub headers: Vec<(String, String)>,
}

impl Response {
    /// First header named `name` (case-insensitive).
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }

    pub fn json(&self) -> anyhow::Result<Value> {
        Ok(serde_json::from_str(&self.body)?)
    }

    /// Error unless 2xx, with the body for context.
    pub fn ok(self) -> anyhow::Result<Response> {
        if (200..300).contains(&self.status) {
            Ok(self)
        } else {
            let snippet: String = self.body.chars().take(2000).collect();
            anyhow::bail!("HTTP {}: {}", self.status, snippet)
        }
    }
}

pub trait Http: Send + Sync {
    /// `body` is sent as-is; set a content-type header when you send one.
    fn request(&self, method: &str, url: &str, headers: &[(&str, &str)], body: Option<&str>) -> anyhow::Result<Response>;

    fn get(&self, url: &str, headers: &[(&str, &str)]) -> anyhow::Result<Response> {
        self.request("GET", url, headers, None)
    }

    fn post_json(&self, url: &str, headers: &[(&str, &str)], body: &Value) -> anyhow::Result<Response> {
        let mut h = headers.to_vec();
        h.push(("content-type", "application/json"));
        self.request("POST", url, &h, Some(&body.to_string()))
    }
}

pub struct UreqHttp {
    agent: ureq::Agent,
}

impl Default for UreqHttp {
    fn default() -> Self {
        UreqHttp::with_timeout(30)
    }
}

impl UreqHttp {
    /// Whole-request timeout. LLM calls need longer than the 30 s default.
    pub fn with_timeout(secs: u64) -> Self {
        let agent = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .timeout_global(Some(std::time::Duration::from_secs(secs)))
            .build()
            .new_agent();
        UreqHttp { agent }
    }

    /// A caller-built agent (e.g. Obsidian's, which pins the plugin's certificate
    /// and never goes through a proxy). It must keep `http_status_as_error(false)`.
    pub fn from_agent(agent: ureq::Agent) -> Self {
        UreqHttp { agent }
    }
}

impl Http for UreqHttp {
    fn request(&self, method: &str, url: &str, headers: &[(&str, &str)], body: Option<&str>) -> anyhow::Result<Response> {
        let method: ureq::http::Method = method.parse()?;
        let mut req = ureq::http::Request::builder().method(method).uri(url);
        for (k, v) in headers {
            req = req.header(*k, *v);
        }
        let mut resp = match body {
            Some(b) => self.agent.run(req.body(b.to_string())?)?,
            None => self.agent.run(req.body(())?)?,
        };
        let status = resp.status().as_u16();
        let headers = resp
            .headers()
            .iter()
            .filter_map(|(k, v)| Some((k.as_str().to_ascii_lowercase(), v.to_str().ok()?.to_owned())))
            .collect();
        let body = resp.body_mut().read_to_string()?;
        Ok(Response { status, body, headers })
    }
}

/// Test double: returns queued responses for URLs containing a given substring,
/// and records every request.
#[derive(Default)]
pub struct FakeHttp {
    routes: std::sync::Mutex<Vec<(String, String, Response)>>,
    pub log: std::sync::Mutex<Vec<(String, String, Option<String>)>>,
    /// Request headers, parallel to `log` (names as the caller wrote them).
    pub header_log: std::sync::Mutex<Vec<Vec<(String, String)>>>,
}

impl FakeHttp {
    /// Respond to `method` requests whose URL contains `url_part`. Later routes win.
    pub fn on(&self, method: &str, url_part: &str, status: u16, body: impl Into<String>) -> &Self {
        self.on_response(method, url_part, Response { status, body: body.into(), headers: vec![] })
    }

    /// Like `on`, with full control over the response (e.g. headers).
    pub fn on_response(&self, method: &str, url_part: &str, resp: Response) -> &Self {
        self.routes.lock().unwrap().push((method.into(), url_part.into(), resp));
        self
    }

    pub fn requests(&self) -> Vec<(String, String, Option<String>)> {
        self.log.lock().unwrap().clone()
    }
}

impl Http for FakeHttp {
    fn request(&self, method: &str, url: &str, headers: &[(&str, &str)], body: Option<&str>) -> anyhow::Result<Response> {
        self.log.lock().unwrap().push((method.into(), url.into(), body.map(str::to_owned)));
        self.header_log.lock().unwrap().push(headers.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect());
        let routes = self.routes.lock().unwrap();
        routes
            .iter()
            .rev()
            .find(|(m, part, _)| m == method && url.contains(part.as_str()))
            .map(|(_, _, r)| r.clone())
            .ok_or_else(|| anyhow::anyhow!("FakeHttp: no route for {method} {url}"))
    }
}
