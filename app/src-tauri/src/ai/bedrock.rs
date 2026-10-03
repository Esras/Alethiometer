//! Amazon Bedrock Converse API over our own `Http`, signed with SigV4.
//!
//! `POST https://bedrock-runtime.<region>.amazonaws.com/model/<modelId>/converse`
//! (IAM action `bedrock:InvokeModel`). Request and response shapes follow the
//! `bedrock-runtime` service model (botocore `bedrock-runtime/2023-09-30`).
//!
//! Routing is US-only, fail closed (`check_route`): the region must be a
//! commercial `us-*` region and the model a `us.` inference profile, so Bedrock
//! keeps the request inside US regions (the user's BAA covers Bedrock there).
//! Bedrock's separate `bedrock-mantle` endpoint is not used.
//!
//! Two model families are called, told apart by id prefix (`family`):
//! `us.anthropic.*` (e.g. Opus 5.5) and `us.openai.*` (e.g. GPT-6 Luna). Both
//! take the same Converse body (`system`, one user message,
//! `inferenceConfig.maxTokens`). They differ only in the optional reasoning
//! effort, which goes in `additionalModelRequestFields` under a vendor-specific
//! key. Both can put a `reasoningContent` block before the answer (Claude:
//! `reasoningText`; GPT-6: `redactedContent`); only `text` blocks are the answer.
//! We never send `temperature`, `topP` or `stopSequences`: GPT-6 models reject
//! them on Converse, and Opus 5.5 rejects sampling parameters.
//!
//! Signing uses AWS's own `aws-sigv4` crate rather than the full AWS SDK: the
//! SDK would pull in tokio, hyper and a second HTTP stack for one POST, while
//! hand-rolling SigV4 is easy to get subtly wrong (path double-encoding,
//! header canonicalization). The known-answer tests below pin it against
//! botocore and the AWS SigV4 test suite.

use super::creds::AwsCreds;
use crate::sync::http::Http;
use serde_json::{json, Value};
use std::time::SystemTime;

pub struct Request<'a> {
    pub region: &'a str,
    /// A `us.` inference-profile id (see `check_route`).
    pub model: &'a str,
    pub system: &'a str,
    pub user: &'a str,
    /// Caps reasoning plus answer: both families count reasoning tokens here.
    pub max_tokens: u32,
    /// None = the model's own default (medium for both Opus 5.5 and GPT-6 Luna).
    pub effort: Option<Effort>,
}

/// Model families Alethiometer knows the request shape for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Family {
    Anthropic,
    OpenAi,
}

/// The explicit id-prefix mapping. Anything else is unknown and fails closed.
pub fn family(model: &str) -> Option<Family> {
    if model.starts_with("us.anthropic.") {
        Some(Family::Anthropic)
    } else if model.starts_with("us.openai.") {
        Some(Family::OpenAi)
    } else {
        None
    }
}

/// Reasoning effort. Both families accept more levels (none/xhigh/max); these
/// three are valid on both and cover what this app needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Effort {
    Low,
    Medium,
    High,
}

impl Effort {
    /// "" means "not set". Anything else unknown is an error, not a silent default.
    pub fn parse(s: &str) -> anyhow::Result<Option<Effort>> {
        match s.trim() {
            "" => Ok(None),
            "low" => Ok(Some(Effort::Low)),
            "medium" => Ok(Some(Effort::Medium)),
            "high" => Ok(Some(Effort::High)),
            other => anyhow::bail!("effort '{other}' isn't supported (use low, medium or high)"),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Effort::Low => "low",
            Effort::Medium => "medium",
            Effort::High => "high",
        }
    }
}

/// Where the effort goes in `additionalModelRequestFields`, per family:
/// - Claude: `output_config.effort` (Anthropic's own field, passed through by
///   Converse). Opus 5.5 always thinks adaptively, so no `thinking` field is sent.
/// - OpenAI GPT-6 / GPT-5.6: `reasoning.effort`. Converse rejects the flat
///   `reasoning_effort` with 400 unknown_parameter.
fn effort_fields(family: Family, effort: Effort) -> Value {
    match family {
        Family::Anthropic => json!({ "output_config": { "effort": effort.as_str() } }),
        Family::OpenAi => json!({ "reasoning": { "effort": effort.as_str() } }),
    }
}

/// Fail closed unless the call stays in the US: a commercial `us-*` region and
/// a `us.` inference-profile id of a known family. Runs before credentials are
/// read or anything is sent.
pub fn check_route(region: &str, model: &str) -> anyhow::Result<Family> {
    if region.starts_with("us-gov-") {
        anyhow::bail!(
            "ai.region '{region}' is AWS GovCloud, a separate partition Alethiometer doesn't support. \
             Use a commercial US region such as us-east-1. Nothing was sent"
        );
    }
    if !region.starts_with("us-") {
        anyhow::bail!(
            "ai.region '{region}' is not a US region. Alethiometer only sends work data to Bedrock in US regions \
             (e.g. us-east-1, us-west-2), which is what the BAA covers. Nothing was sent"
        );
    }
    if !model.starts_with("us.") {
        let hint = match model.split_once('.') {
            Some(("anthropic" | "openai", _)) => format!(" Use us.{model}."),
            Some((geo @ ("global" | "eu" | "apac" | "jp" | "au" | "ca"), rest)) => {
                format!(" '{geo}.' profiles can route outside the US; use us.{rest}.")
            }
            _ => String::new(),
        };
        anyhow::bail!(
            "Model '{model}' is not a US inference profile. Alethiometer only calls ids starting with 'us.' \
             (e.g. us.anthropic.claude-opus-5-5, us.openai.gpt-6-luna), which keep requests in US regions.{hint} \
             Nothing was sent"
        );
    }
    family(model).ok_or_else(|| {
        anyhow::anyhow!(
            "Model '{model}' is from a vendor Alethiometer doesn't know the request shape for; \
             it calls us.anthropic.* and us.openai.* models. Nothing was sent"
        )
    })
}

#[derive(Debug, Clone)]
pub struct Reply {
    pub text: String,
    pub stop_reason: String,
    pub input_tokens: i64,
    pub output_tokens: i64,
}

impl Reply {
    pub fn answer(self) -> anyhow::Result<String> {
        if self.text.is_empty() {
            let why = if self.stop_reason.is_empty() { "none" } else { &self.stop_reason };
            // Models that think by default can spend the whole budget before answering.
            let hint = if self.stop_reason == "max_tokens" { "; the model used its whole token budget, often on reasoning" } else { "" };
            anyhow::bail!("Bedrock returned no text (stop reason: {why}{hint})");
        }
        Ok(self.text)
    }
}

pub fn endpoint(region: &str, model: &str) -> anyhow::Result<String> {
    // The region becomes part of a hostname; only allow what AWS region names use.
    let ok = !region.is_empty() && region.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
    if !ok {
        anyhow::bail!("ai.region '{region}' doesn't look like an AWS region (e.g. us-east-1)");
    }
    Ok(format!("https://bedrock-runtime.{region}.amazonaws.com/model/{}/converse", encode_label(model)))
}

/// Percent-encode a URI path label (model ids contain ':' and ARNs contain '/').
fn encode_label(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

pub fn body(req: &Request) -> Value {
    let mut b = json!({
        "system": [{ "text": req.system }],
        "messages": [{ "role": "user", "content": [{ "text": req.user }] }],
        "inferenceConfig": { "maxTokens": req.max_tokens },
    });
    if let (Some(effort), Some(fam)) = (req.effort, family(req.model)) {
        b["additionalModelRequestFields"] = effort_fields(fam, effort);
    }
    b
}

/// One Converse call. Errors are user-facing and never contain credentials.
pub fn converse(http: &dyn Http, creds: &AwsCreds, req: &Request, time: SystemTime) -> anyhow::Result<Reply> {
    check_route(req.region, req.model)?;
    let url = endpoint(req.region, req.model)?;
    let body = body(req).to_string();
    let base = [("content-type", "application/json")];
    let mut headers: Vec<(String, String)> = base.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
    match creds {
        AwsCreds::Bearer { token, .. } => headers.push(("authorization".to_string(), format!("Bearer {token}"))),
        AwsCreds::Keys { .. } => {
            let signed = sign_headers(creds, req.region, "bedrock", "POST", &url, &base, body.as_bytes(), time)?;
            headers.extend(signed);
        }
    }
    let headers: Vec<(&str, &str)> = headers.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
    let resp = http
        .request("POST", &url, &headers, Some(&body))
        .map_err(|e| anyhow::anyhow!("Could not reach Bedrock in {}: {e}", req.region))?;
    if !(200..300).contains(&resp.status) {
        return Err(map_error(resp.status, &resp.body, req));
    }
    parse_reply(&resp.body)
}

pub fn parse_reply(body: &str) -> anyhow::Result<Reply> {
    let v: Value = serde_json::from_str(body).map_err(|_| anyhow::anyhow!("Bedrock returned a response Alethiometer couldn't read"))?;
    let stop_reason = v.get("stopReason").and_then(Value::as_str).unwrap_or("").to_string();
    // Only `text` blocks are the answer. `reasoningContent` (Claude's
    // `reasoningText`, GPT-6's opaque `redactedContent`) is skipped.
    let text = v
        .pointer("/output/message/content")
        .and_then(Value::as_array)
        .map(|blocks| blocks.iter().filter_map(|b| b.get("text").and_then(Value::as_str)).collect::<Vec<_>>().join(""))
        .unwrap_or_default();
    let text = text.trim().to_string();
    let tokens = |k: &str| v.pointer(&format!("/usage/{k}")).and_then(Value::as_i64).unwrap_or(0);
    Ok(Reply { text, stop_reason, input_tokens: tokens("inputTokens"), output_tokens: tokens("outputTokens") })
}

/// Turn a Bedrock error response into a message the user can act on.
pub fn map_error(status: u16, body: &str, req: &Request) -> anyhow::Error {
    let aws: String = serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|v| v.get("message").or_else(|| v.get("Message")).and_then(Value::as_str).map(str::to_owned))
        .unwrap_or_else(|| body.to_string())
        .chars()
        .take(300)
        .collect();
    let lower = aws.to_lowercase();
    let (region, model) = (req.region, req.model);
    match status {
        403 if lower.contains("signature") || lower.contains("security token") || lower.contains("expired") => anyhow::anyhow!(
            "Bedrock rejected the AWS credentials (403). Check the keys or refresh the session. AWS said: {aws}"
        ),
        401 | 403 => anyhow::anyhow!(
            "Bedrock denied access to {model} (403): model access not enabled in Bedrock console for {region}, \
             or the IAM policy lacks bedrock:InvokeModel on this model/inference profile. AWS said: {aws}"
        ),
        429 => anyhow::anyhow!("Bedrock is throttling requests or the model isn't ready (429). Try again in a minute. AWS said: {aws}"),
        404 => anyhow::anyhow!("Bedrock doesn't know model '{model}' in {region} (404). AWS said: {aws}"),
        400 if lower.contains("on-demand throughput") || lower.contains("inference profile") => anyhow::anyhow!(
            "Bedrock needs an inference-profile id for {model} in {region} (400); check the id under \
             Bedrock → Cross-region inference. AWS said: {aws}"
        ),
        400 => anyhow::anyhow!("Bedrock rejected the request (400). AWS said: {aws}"),
        408 | 424 | 500..=599 => anyhow::anyhow!("Bedrock had a temporary problem (HTTP {status}). Try again. AWS said: {aws}"),
        _ => anyhow::anyhow!("Bedrock returned HTTP {status}. AWS said: {aws}"),
    }
}

/// SigV4 headers (`authorization`, `x-amz-date`, `x-amz-security-token`) for a request.
/// Every header in `headers` is signed; `host` comes from the URL.
#[allow(clippy::too_many_arguments)]
pub fn sign_headers(
    creds: &AwsCreds,
    region: &str,
    service: &str,
    method: &str,
    url: &str,
    headers: &[(&str, &str)],
    body: &[u8],
    time: SystemTime,
) -> anyhow::Result<Vec<(String, String)>> {
    use aws_sigv4::http_request::{sign, SignableBody, SignableRequest, SigningSettings};
    use aws_sigv4::sign::v4;
    let AwsCreds::Keys { access_key_id, secret_access_key, session_token, .. } = creds else {
        anyhow::bail!("sign_headers called with bearer credentials")
    };
    let identity = aws_credential_types::Credentials::new(access_key_id, secret_access_key, session_token.clone(), None, "alethiometer").into();
    let params = v4::SigningParams::builder()
        .identity(&identity)
        .region(region)
        .name(service)
        .time(time)
        .settings(SigningSettings::default())
        .build()
        .map_err(|e| anyhow::anyhow!("SigV4 setup failed: {e}"))?
        .into();
    let signable = SignableRequest::new(method, url, headers.iter().copied(), SignableBody::Bytes(body))
        .map_err(|e| anyhow::anyhow!("SigV4 couldn't sign this request: {e}"))?;
    let (instructions, _sig) = sign(signable, &params).map_err(|e| anyhow::anyhow!("SigV4 signing failed: {e}"))?.into_parts();
    Ok(instructions.headers().map(|(k, v)| (k.to_string(), v.to_string())).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync::http::{FakeHttp, Response};
    use std::sync::Mutex;

    const SECRET: &str = "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY";

    fn creds(token: Option<&str>) -> AwsCreds {
        AwsCreds::Keys { access_key_id: "AKIDEXAMPLE".into(), secret_access_key: SECRET.into(), session_token: token.map(str::to_owned), source: "test".into() }
    }

    fn at(rfc3339: &str) -> SystemTime {
        SystemTime::try_from(rfc3339.parse::<jiff::Timestamp>().unwrap()).unwrap()
    }

    fn header<'a>(h: &'a [(String, String)], name: &str) -> &'a str {
        h.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str()).unwrap_or("")
    }

    /// AWS SigV4 test suite, "get-vanilla" (published expected signature).
    #[test]
    fn sigv4_known_answer_aws_suite_get_vanilla() {
        let h = sign_headers(&creds(None), "us-east-1", "service", "GET", "https://example.amazonaws.com/", &[], b"", at("2015-08-30T12:36:00Z")).unwrap();
        assert_eq!(
            header(&h, "authorization"),
            "AWS4-HMAC-SHA256 Credential=AKIDEXAMPLE/20150830/us-east-1/service/aws4_request, SignedHeaders=host;x-amz-date, \
             Signature=5fa00fa31553b73ebf1942676e86291e8372ff2a2260956d9b8aae1d763fbf31"
        );
        assert_eq!(header(&h, "x-amz-date"), "20150830T123600Z");
    }

    /// The exact Converse request shape (encoded ':' in the model id, session
    /// token, JSON body). Expected value computed with botocore 1.43 SigV4Auth.
    #[test]
    fn sigv4_known_answer_bedrock_converse_matches_botocore() {
        let url = endpoint("us-east-1", "us.anthropic.claude-haiku-4-5-20251001-v1:0").unwrap();
        assert_eq!(url, "https://bedrock-runtime.us-east-1.amazonaws.com/model/us.anthropic.claude-haiku-4-5-20251001-v1%3A0/converse");
        let body = r#"{"messages":[{"role":"user","content":[{"text":"hi"}]}]}"#;
        let h = sign_headers(
            &creds(Some("SESSIONTOKENEXAMPLE")),
            "us-east-1",
            "bedrock",
            "POST",
            &url,
            &[("content-type", "application/json")],
            body.as_bytes(),
            at("2026-09-27T12:00:00Z"),
        )
        .unwrap();
        assert_eq!(
            header(&h, "authorization"),
            "AWS4-HMAC-SHA256 Credential=AKIDEXAMPLE/20260927/us-east-1/bedrock/aws4_request, \
             SignedHeaders=content-type;host;x-amz-date;x-amz-security-token, \
             Signature=7e775684888da9e9b68eb1a4b5393b8dd4dced74f129c85b7a08be0dbfa93ef8"
        );
        assert_eq!(header(&h, "x-amz-security-token"), "SESSIONTOKENEXAMPLE");
    }

    /// `endpoint` only builds the URL (`check_route` decides what may be called).
    #[test]
    fn model_path_labels_are_encoded() {
        let url = endpoint("us-east-1", "arn:aws:bedrock:us-east-1:123456789012:inference-profile/us.anthropic.x-v1:0").unwrap();
        assert!(url.contains("/model/arn%3Aaws%3Abedrock%3Aus-east-1%3A123456789012%3Ainference-profile%2Fus.anthropic.x-v1%3A0/converse"), "{url}");
        assert!(endpoint("evil.com/x", "m").is_err());
    }

    /// Records headers too (FakeHttp only keeps method, URL and body).
    struct HeaderSpy {
        inner: FakeHttp,
        headers: Mutex<Vec<(String, String)>>,
    }

    impl Http for HeaderSpy {
        fn request(&self, method: &str, url: &str, headers: &[(&str, &str)], body: Option<&str>) -> anyhow::Result<Response> {
            *self.headers.lock().unwrap() = headers.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
            self.inner.request(method, url, headers, body)
        }
    }

    fn req<'a>() -> Request<'a> {
        Request { region: "us-east-1", model: "us.anthropic.claude-sonnet-4-6", system: "SYS", user: "USER", max_tokens: 256, effort: None }
    }

    #[test]
    fn converse_request_and_response_mapping() {
        let spy = HeaderSpy { inner: FakeHttp::default(), headers: Mutex::new(vec![]) };
        spy.inner.on("POST", "/model/us.anthropic.claude-sonnet-4-6/converse", 200, include_str!("../../tests/fixtures/bedrock/converse_ok.json"));
        let r = converse(&spy, &creds(Some("TOK")), &req(), SystemTime::now()).unwrap();
        assert_eq!(r.text, "The renewal is due Friday; Kim owns the quote.");
        assert_eq!(r.stop_reason, "end_turn");

        let log = spy.inner.requests();
        assert_eq!(log.len(), 1);
        assert_eq!(log[0].1, "https://bedrock-runtime.us-east-1.amazonaws.com/model/us.anthropic.claude-sonnet-4-6/converse");
        let sent: Value = serde_json::from_str(log[0].2.as_deref().unwrap()).unwrap();
        assert_eq!(sent["system"][0]["text"], "SYS");
        assert_eq!(sent["messages"][0]["role"], "user");
        assert_eq!(sent["messages"][0]["content"][0]["text"], "USER");
        assert_eq!(sent["inferenceConfig"]["maxTokens"], 256);
        // Nothing either family rejects, and no effort unless one is set.
        assert_eq!(sent["inferenceConfig"].as_object().unwrap().len(), 1);
        assert!(sent.get("additionalModelRequestFields").is_none());

        let h = spy.headers.lock().unwrap().clone();
        assert!(header(&h, "authorization").starts_with("AWS4-HMAC-SHA256 Credential=AKIDEXAMPLE/"));
        assert!(!header(&h, "x-amz-date").is_empty());
        assert_eq!(header(&h, "x-amz-security-token"), "TOK");
        assert!(h.iter().all(|(_, v)| !v.contains(SECRET)), "secret key must never be sent");
    }

    fn err_for(status: u16, fixture: &str) -> String {
        let http = FakeHttp::default();
        http.on("POST", "/converse", status, fixture);
        converse(&http, &creds(None), &req(), SystemTime::now()).unwrap_err().to_string()
    }

    #[test]
    fn error_mapping() {
        let e = err_for(403, r#"{"message":"You don't have access to the model with the specified model ID."}"#);
        assert!(e.contains("model access not enabled in Bedrock console for us-east-1"), "{e}");
        let e = err_for(403, r#"{"message":"The security token included in the request is invalid."}"#);
        assert!(e.contains("rejected the AWS credentials"), "{e}");
        let e = err_for(429, r#"{"message":"Too many requests, please wait before trying again."}"#);
        assert!(e.contains("throttling"), "{e}");
        let e = err_for(400, r#"{"message":"Invocation of model ID anthropic.claude-sonnet-4-6 with on-demand throughput isn't supported. Retry your request with the ID or ARN of an inference profile that contains this model."}"#);
        assert!(e.contains("inference-profile id"), "{e}");
        let e = err_for(400, r#"{"message":"The provided model identifier is invalid."}"#);
        assert!(e.contains("rejected the request (400)") && e.contains("model identifier is invalid"), "{e}");
        // Model errors pass through Converse as a 400 ValidationException.
        let e = err_for(400, r#"{"message":"The model returned the following errors: tool_choice: type \"tool\" and \"any\" are not supported for this model."}"#);
        assert!(e.contains("rejected the request (400)") && e.contains("tool_choice"), "{e}");
    }

    fn err_for_model(model: &str, status: u16, fixture: &str) -> String {
        let http = FakeHttp::default();
        http.on("POST", "/converse", status, fixture);
        let r = Request { model, ..req() };
        converse(&http, &creds(None), &r, SystemTime::now()).unwrap_err().to_string()
    }

    /// Non-`us.` ids fail closed before any request, with the `us.` id to use.
    #[test]
    fn non_us_model_ids_fail_closed() {
        for (model, hint) in [
            ("anthropic.claude-opus-5-5", "Use us.anthropic.claude-opus-5-5"),
            ("openai.gpt-6-luna", "Use us.openai.gpt-6-luna"),
            ("global.anthropic.claude-opus-5-5", "use us.anthropic.claude-opus-5-5"),
            ("eu.anthropic.claude-opus-5-5", "route outside the US"),
            ("arn:aws:bedrock:us-east-1:123456789012:inference-profile/us.anthropic.x", "not a US inference profile"),
        ] {
            let e = err_for_model(model, 200, include_str!("../../tests/fixtures/bedrock/converse_ok.json"));
            assert!(e.contains("not a US inference profile") && e.contains(hint) && e.contains("Nothing was sent"), "{model}: {e}");
        }
        // A US profile of a vendor we don't know the request shape for.
        let e = err_for_model("us.meta.llama4-maverick-17b-instruct-v1:0", 200, "{}");
        assert!(e.contains("us.anthropic.* and us.openai.*"), "{e}");
        let e = err_for(503, r#"{"message":"Service unavailable"}"#);
        assert!(e.contains("temporary"), "{e}");
    }

    #[test]
    fn non_us_regions_fail_closed_before_any_request() {
        for region in ["eu-west-1", "ca-central-1", "ap-northeast-1", "us-gov-west-1", ""] {
            let http = FakeHttp::default();
            http.on("POST", "/converse", 200, include_str!("../../tests/fixtures/bedrock/converse_ok.json"));
            let e = converse(&http, &creds(None), &Request { region, ..req() }, SystemTime::now()).unwrap_err().to_string();
            assert!(e.contains("Nothing was sent") && e.contains(&format!("'{region}'")), "{region}: {e}");
            assert!(http.requests().is_empty(), "{region}");
        }
        assert!(check_route("us-west-2", "us.openai.gpt-6-luna").is_ok());
    }

    #[test]
    fn family_mapping_is_by_prefix() {
        assert_eq!(family("us.anthropic.claude-opus-5-5"), Some(Family::Anthropic));
        assert_eq!(family("us.openai.gpt-6-luna"), Some(Family::OpenAi));
        assert_eq!(family("openai.gpt-6-luna"), None);
        assert_eq!(family("us.amazon.nova-pro-v1:0"), None);
    }

    /// Effort lands under each vendor's own key in additionalModelRequestFields.
    #[test]
    fn effort_goes_where_each_family_expects_it() {
        let claude = body(&Request { model: "us.anthropic.claude-opus-5-5", effort: Some(Effort::High), ..req() });
        assert_eq!(claude["additionalModelRequestFields"], json!({ "output_config": { "effort": "high" } }));
        let gpt = body(&Request { model: "us.openai.gpt-6-luna", effort: Some(Effort::Low), ..req() });
        assert_eq!(gpt["additionalModelRequestFields"], json!({ "reasoning": { "effort": "low" } }));
        assert!(gpt["additionalModelRequestFields"].get("reasoning_effort").is_none());
        assert_eq!(Effort::parse("").unwrap(), None);
        assert_eq!(Effort::parse(" medium ").unwrap(), Some(Effort::Medium));
        assert!(Effort::parse("max").is_err());
    }

    /// GPT-6 on Converse returns its reasoning as an opaque `redactedContent`
    /// block before the text (shape from a live us-east-1 call reported in
    /// langfuse#17857). Only the text is the answer.
    #[test]
    fn openai_reply_with_redacted_reasoning_extracts_only_the_text() {
        let body = r#"{
          "output": {"message": {"role": "assistant", "content": [
            {"reasoningContent": {"redactedContent": "cnNuX3J1dTZCZXhhbXBsZQ=="}},
            {"text": "{\"topic_id\": 3, \"confidence\": 0.82, \"reason\": \"Same vendor renewal\"}"}
          ]}},
          "stopReason": "end_turn",
          "usage": {"inputTokens": 310, "outputTokens": 96, "totalTokens": 406},
          "metrics": {"latencyMs": 880}
        }"#;
        let http = FakeHttp::default();
        http.on("POST", "/model/us.openai.gpt-6-luna/converse", 200, body);
        let r = converse(&http, &creds(None), &Request { model: "us.openai.gpt-6-luna", ..req() }, SystemTime::now()).unwrap();
        assert_eq!(r.text, r#"{"topic_id": 3, "confidence": 0.82, "reason": "Same vendor renewal"}"#);
        assert_eq!(r.stop_reason, "end_turn");
        // Reasoning that ate the whole budget is an error, not a blank answer.
        let cut = r#"{"output":{"message":{"role":"assistant","content":[{"reasoningContent":{"redactedContent":"eA=="}}]}},"stopReason":"max_tokens"}"#;
        let e = parse_reply(cut).unwrap().answer().unwrap_err().to_string();
        assert!(e.contains("max_tokens") && e.contains("reasoning"), "{e}");
    }

    /// Both families use bedrock-runtime Converse with dateless `us.` profile
    /// ids: same endpoint, same signing scope.
    #[test]
    fn us_profiles_of_both_families_use_converse_and_bedrock_scope() {
        for model in ["us.anthropic.claude-opus-5-5", "us.openai.gpt-6-luna", "us.anthropic.claude-opus-4-8"] {
            let spy = HeaderSpy { inner: FakeHttp::default(), headers: Mutex::new(vec![]) };
            spy.inner.on("POST", "/converse", 200, include_str!("../../tests/fixtures/bedrock/converse_ok.json"));
            converse(&spy, &creds(None), &Request { model, ..req() }, at("2026-09-27T12:00:00Z")).unwrap();
            assert_eq!(spy.inner.requests()[0].1, format!("https://bedrock-runtime.us-east-1.amazonaws.com/model/{model}/converse"));
            let h = spy.headers.lock().unwrap().clone();
            assert!(header(&h, "authorization").contains("/20260927/us-east-1/bedrock/aws4_request"), "{model}");
        }
    }

    /// Models that think by default (Sonnet 5, Opus 5.x) return a
    /// `reasoningContent` block before the text; only the text is the answer.
    #[test]
    fn reasoning_blocks_are_not_part_of_the_answer() {
        let r = parse_reply(include_str!("../../tests/fixtures/bedrock/converse_reasoning.json")).unwrap();
        assert_eq!(r.text, "Two open items: the Datadog quote and the Acme postmortem.");
        assert_eq!(r.stop_reason, "end_turn");
    }

    #[test]
    fn empty_output_is_an_error_not_a_blank_answer() {
        let body = r#"{"output":{"message":{"role":"assistant","content":[]}},"stopReason":"content_filtered","usage":{"inputTokens":1,"outputTokens":0,"totalTokens":1},"metrics":{"latencyMs":5}}"#;
        let r = parse_reply(body).unwrap();
        assert_eq!((r.input_tokens, r.output_tokens), (1, 0));
        let e = r.answer().unwrap_err().to_string();
        assert!(e.contains("content_filtered"), "{e}");
        let body = r#"{"output":{"message":{"role":"assistant","content":[{"reasoningContent":{"reasoningText":{"text":"","signature":"x"}}}]}},"stopReason":"max_tokens"}"#;
        let e = parse_reply(body).unwrap().answer().unwrap_err().to_string();
        assert!(e.contains("max_tokens") && e.contains("reasoning"), "{e}");
    }
}
