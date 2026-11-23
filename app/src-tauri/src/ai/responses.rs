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
    /// Input items after the user message (earlier calls and their outputs), as `Reply::turn` and `results` make them.
    pub history: &'a [Value],
    pub tools: &'a [super::ToolDef],
}

pub fn body(req: &Request) -> Value {
    let input = if req.history.is_empty() {
        json!(req.user)
    } else {
        let mut items = vec![json!({ "role": "user", "content": req.user })];
        items.extend(req.history.iter().cloned());
        Value::Array(items)
    };
    let mut b = json!({
        "model": req.model,
        "instructions": req.system,
        "input": input,
        "max_output_tokens": req.max_tokens,
        "store": false,
    });
    if let Some(effort) = req.effort {
        b["reasoning"] = json!({ "effort": effort.as_str() });
    }
    if !req.tools.is_empty() {
        b["tools"] = req
            .tools
            .iter()
            .map(|t| json!({ "type": "function", "name": t.name, "description": t.description, "parameters": t.schema() }))
            .collect();
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
            .and_then(|v| {
                v.pointer("/error/message")
                    .or_else(|| v.get("message"))
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            })
            .unwrap_or(resp.body)
            .chars()
            .take(300)
            .collect();
        anyhow::bail!(
            "{} returned HTTP {} for {}: {detail}",
            req.provider,
            resp.status,
            req.model
        );
    }
    let reply = parse_reply(&resp.body)
        .map_err(|_| anyhow::anyhow!("{} returned a response Alethiometer couldn't read", req.provider))?;
    if reply.text.is_empty() && reply.calls.is_empty() {
        let why = if reply.stop_reason.is_empty() {
            "none"
        } else {
            &reply.stop_reason
        };
        anyhow::bail!("{} returned no text (stop reason: {why})", req.provider);
    }
    Ok(reply)
}

pub fn parse_reply(body: &str) -> anyhow::Result<Reply> {
    let v: Value = serde_json::from_str(body)?;
    let output: Vec<Value> = v.get("output").and_then(Value::as_array).cloned().unwrap_or_default();
    let text = output
        .iter()
        .filter_map(|item| item.get("content").and_then(Value::as_array))
        .flatten()
        .filter(|part| part.get("type").and_then(Value::as_str) == Some("output_text"))
        .filter_map(|part| part.get("text").and_then(Value::as_str))
        .collect::<String>();
    let calls: Vec<Value> = output
        .into_iter()
        .filter(|i| i.get("type").and_then(Value::as_str) == Some("function_call"))
        .collect();
    let stop_reason = v
        .pointer("/incomplete_details/reason")
        .or_else(|| v.get("status"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let tokens = |k: &str| v.pointer(&format!("/usage/{k}")).and_then(Value::as_i64).unwrap_or(0);
    Ok(Reply {
        text: text.trim().to_string(),
        stop_reason,
        input_tokens: tokens("input_tokens"),
        output_tokens: tokens("output_tokens"),
        calls: calls
            .iter()
            .filter_map(|c| {
                Some(super::ToolCall {
                    id: c.get("call_id")?.as_str()?.to_string(),
                    name: c.get("name")?.as_str()?.to_string(),
                    input: serde_json::from_str(c.get("arguments")?.as_str()?).unwrap_or(Value::Null),
                })
            })
            .collect(),
        // Only the calls go back, without their item ids: with `store: false` nothing can be referred to by id.
        turn: calls
            .iter()
            .map(|c| json!({ "type": "function_call", "call_id": c["call_id"], "name": c["name"], "arguments": c["arguments"] }))
            .collect(),
    })
}

/// Tool outputs as input items answering the model's `function_call`s: (call id, text).
pub fn results(outputs: &[(String, String)]) -> Vec<Value> {
    outputs
        .iter()
        .map(|(id, text)| json!({ "type": "function_call_output", "call_id": id, "output": text }))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn function_calls_parse_and_go_back_without_item_ids() {
        let r = parse_reply(
            r#"{"output": [
                {"type": "reasoning", "id": "rs_1", "content": [{"type": "reasoning_text", "text": "think"}]},
                {"type": "function_call", "id": "fc_1", "call_id": "c1", "name": "get_item", "arguments": "{\"item_id\": 7}"}
            ], "status": "completed"}"#,
        )
        .unwrap();
        assert_eq!(r.calls.len(), 1);
        assert_eq!(
            (r.calls[0].id.as_str(), r.calls[0].input["item_id"].as_i64()),
            ("c1", Some(7))
        );
        assert_eq!(
            r.turn,
            vec![
                json!({"type": "function_call", "call_id": "c1", "name": "get_item", "arguments": "{\"item_id\": 7}"})
            ]
        );
        let back = results(&[("c1".into(), "text".into())]);
        assert_eq!(back[0]["type"], "function_call_output");
    }
}
