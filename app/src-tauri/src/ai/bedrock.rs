//! Amazon Bedrock Converse API over our own `Http`, signed with SigV4.
//!
//! `POST https://bedrock-runtime.<region>.amazonaws.com/model/<modelId>/converse`
//! (IAM action `bedrock:InvokeModel`). Request and response shapes follow the
//! `bedrock-runtime` service model (botocore `bedrock-runtime/2023-09-30`).
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
    /// Model id, inference-profile id, or ARN.
    pub model: &'a str,
    pub system: &'a str,
    pub user: &'a str,
    pub max_tokens: u32,
}

#[derive(Debug, Clone)]
pub struct Reply {
    pub text: String,
    pub stop_reason: String,
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
    json!({
        "system": [{ "text": req.system }],
        "messages": [{ "role": "user", "content": [{ "text": req.user }] }],
        "inferenceConfig": { "maxTokens": req.max_tokens },
    })
}

/// One Converse call. Errors are user-facing and never contain credentials.
pub fn converse(http: &dyn Http, creds: &AwsCreds, req: &Request, time: SystemTime) -> anyhow::Result<Reply> {
    let url = endpoint(req.region, req.model)?;
    let body = body(req).to_string();
    let base = [("content-type", "application/json")];
    let signed = sign_headers(creds, req.region, "bedrock", "POST", &url, &base, body.as_bytes(), time)?;
    let mut headers: Vec<(&str, &str)> = base.to_vec();
    headers.extend(signed.iter().map(|(k, v)| (k.as_str(), v.as_str())));
    let resp = http
        .request("POST", &url, &headers, Some(&body))
        .map_err(|e| anyhow::anyhow!("Could not reach Bedrock in {}: {e}", req.region))?;
    if !(200..300).contains(&resp.status) {
        return Err(map_error(resp.status, &resp.body, req));
    }
    parse_reply(&resp.body)
}

pub fn parse_reply(body: &str) -> anyhow::Result<Reply> {
    let v: Value = serde_json::from_str(body).map_err(|_| anyhow::anyhow!("Bedrock returned a response Helm couldn't read"))?;
    let stop_reason = v.get("stopReason").and_then(Value::as_str).unwrap_or("").to_string();
    let text = v
        .pointer("/output/message/content")
        .and_then(Value::as_array)
        .map(|blocks| blocks.iter().filter_map(|b| b.get("text").and_then(Value::as_str)).collect::<Vec<_>>().join(""))
        .unwrap_or_default();
    let text = text.trim().to_string();
    if text.is_empty() {
        anyhow::bail!("Bedrock returned no text (stop reason: {})", if stop_reason.is_empty() { "none" } else { &stop_reason });
    }
    Ok(Reply { text, stop_reason })
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
            "Bedrock needs an inference-profile id for this model (400); try a 'us.' or 'global.' prefix, \
             e.g. us.{model}. AWS said: {aws}"
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
    let identity = aws_credential_types::Credentials::new(
        &creds.access_key_id,
        &creds.secret_access_key,
        creds.session_token.clone(),
        None,
        "helm",
    )
    .into();
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
    use std::time::{Duration, UNIX_EPOCH};

    const SECRET: &str = "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY";

    fn creds(token: Option<&str>) -> AwsCreds {
        AwsCreds {
            access_key_id: "AKIDEXAMPLE".into(),
            secret_access_key: SECRET.into(),
            session_token: token.map(str::to_owned),
            source: "test".into(),
        }
    }

    fn at(rfc3339: &str) -> SystemTime {
        let secs = chrono::DateTime::parse_from_rfc3339(rfc3339).unwrap().timestamp() as u64;
        UNIX_EPOCH + Duration::from_secs(secs)
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

    #[test]
    fn arn_model_ids_are_path_encoded() {
        let url = endpoint("eu-west-1", "arn:aws:bedrock:eu-west-1:123456789012:inference-profile/eu.anthropic.x-v1:0").unwrap();
        assert!(url.contains("/model/arn%3Aaws%3Abedrock%3Aeu-west-1%3A123456789012%3Ainference-profile%2Feu.anthropic.x-v1%3A0/converse"), "{url}");
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
        Request { region: "us-east-1", model: "us.anthropic.claude-sonnet-4-6", system: "SYS", user: "USER", max_tokens: 256 }
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
        let e = err_for(503, r#"{"message":"Service unavailable"}"#);
        assert!(e.contains("temporary"), "{e}");
    }

    #[test]
    fn empty_output_is_an_error_not_a_blank_answer() {
        let body = r#"{"output":{"message":{"role":"assistant","content":[]}},"stopReason":"content_filtered","usage":{"inputTokens":1,"outputTokens":0,"totalTokens":1},"metrics":{"latencyMs":5}}"#;
        let e = parse_reply(body).unwrap_err().to_string();
        assert!(e.contains("content_filtered"), "{e}");
    }
}
