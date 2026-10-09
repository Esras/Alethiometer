use super::*;
use serde_json::json;

#[test]
fn the_real_configs_validate() {
    // Shapes from docs/sources and the work Mac's settings.json.
    validate(
        "slack",
        &json!({ "channels": "all", "lookback_hours": 12, "muted": ["#random"] }),
    )
    .unwrap();
    validate("slack", &json!({ "channels": ["#inc-2291", "C0123"] })).unwrap();
    validate(
        "gmail",
        &json!({ "account": "me@work.com", "category_labels": { "work": "Work" } }),
    )
    .unwrap();
    validate(
        "gcal",
        &json!({ "category_by_color": { "3": "self" }, "hold_hours": [9, 17] }),
    )
    .unwrap();
    validate(
        "jira",
        &json!({ "base_url": "https://x.atlassian.net", "email": "a@b", "jql": "project = X" }),
    )
    .unwrap();
    validate("github", &json!({ "team_review_requests": false, "read_only": true })).unwrap();
    validate("obsidian", &json!({ "vault": "/v", "write_mode": "file" })).unwrap();
    validate("protoncal", &json!({ "urls": ["https://calendar.proton.me/x"] })).unwrap();
    validate("glean", &json!({ "server_url": "https://acme-be.glean.com" })).unwrap();
    validate(
        "scion",
        &json!({ "hub_url": "https://hub.example.com", "projects": ["ingest-svc"] }),
    )
    .unwrap();
    assert!(validate("scion", &json!({ "hub_url": "http://hub" })).is_err());
    validate("slack", &json!({ "someday_key": 1 })).expect("unknown keys are allowed");
    let e = validate("slack", &json!({ "lookback_hours": "lots" }))
        .unwrap_err()
        .to_string();
    assert!(e.contains("lookback_hours") || e.contains("invalid type"), "{e}");
    assert!(validate("glean", &json!({ "server_url": "http://x" })).is_err());
    assert!(validate("fax", &json!({})).is_err());
}
