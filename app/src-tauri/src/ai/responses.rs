//! OpenAI Responses API (`POST /v1/responses`, bearer key); the URL is fixed per provider in `ai::complete`.

use super::bedrock::{Effort, Reply};
use crate::sync::http::Http;
use serde_json::{json, Value};

pub struct Request<'a> {
    pub provider: &'a str,
    pub url: &'a str,
    pub model: &'a str,
    pub system: &'a str,
    pub user: &'a str,
    pub max_tokens: u32,
    pub effort: Option<Effort>,
}

pub fn body(req: &Request) -> Value {
    let mut b = json!({
        "model": req.model,
        "instructions": req.system,
        "input": req.user,
        "max_output_tokens": req.max_tokens,
        "store": false,
    });
    if let Some(effort) = req.effort {
        b["reasoning"] = json!({ "effort": effort.as_str() });
    }
    b
}

pub fn create(http: &dyn Http, key: &str, req: &Request) -> anyhow::Result<Reply> {
    let auth = format!("Bearer {key}");
    let resp = http
        .post_json(req.url, &[("authorization", &auth)], &body(req))
        .map_err(|e| anyhow::anyhow!("Could not reach {}: {e}", req.provider))?;
    if !(200..300).contains(&resp.status) {
        let detail: String = serde_json::from_str::<Value>(&resp.body)
            .ok()
            .and_then(|v| v.pointer("/error/message").or_else(|| v.get("message")).and_then(Value::as_str).map(str::to_owned))
            .unwrap_or(resp.body)
            .chars()
            .take(300)
            .collect();
        anyhow::bail!("{} returned HTTP {} for {}: {detail}", req.provider, resp.status, req.model);
    }
    let reply = parse_reply(&resp.body).map_err(|_| anyhow::anyhow!("{} returned a response Alethiometer couldn't read", req.provider))?;
    if reply.text.is_empty() {
        let why = if reply.stop_reason.is_empty() { "none" } else { &reply.stop_reason };
        anyhow::bail!("{} returned no text (stop reason: {why})", req.provider);
    }
    Ok(reply)
}

pub fn parse_reply(body: &str) -> anyhow::Result<Reply> {
    let v: Value = serde_json::from_str(body)?;
    let text = v
        .get("output")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|item| item.get("content").and_then(Value::as_array))
        .flatten()
        .filter(|part| part.get("type").and_then(Value::as_str) == Some("output_text"))
        .filter_map(|part| part.get("text").and_then(Value::as_str))
        .collect::<String>();
    let stop_reason =
        v.pointer("/incomplete_details/reason").or_else(|| v.get("status")).and_then(Value::as_str).unwrap_or("").to_string();
    let tokens = |k: &str| v.pointer(&format!("/usage/{k}")).and_then(Value::as_i64).unwrap_or(0);
    Ok(Reply { text: text.trim().to_string(), stop_reason, input_tokens: tokens("input_tokens"), output_tokens: tokens("output_tokens") })
}
