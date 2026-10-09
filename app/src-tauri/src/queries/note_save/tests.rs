use super::*;
use crate::settings::{Settings, SourceConfig};
use crate::sync::http::FakeHttp;
use rusqlite::params;

/// Sample data plus a real obsidian source on the sample vault.
pub(crate) fn app_with_source(write_mode: &str) -> (App, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let app = App::from_parts(crate::db::open_in_memory().unwrap(), Settings::default(), dir.path()).unwrap();
    // Configured after seeding (sources in settings would skip the sample data).
    let mut cfgs = vec![SourceConfig {
        kind: "obsidian".into(),
        tag: "work".into(),
        name: "Work vault".into(),
        enabled: true,
        poll_secs: 60,
        config: json!({ "write_mode": write_mode }),
    }];
    crate::sync::connectors::obsidian::resolve_config(&mut cfgs, &app.vault);
    crate::sync::register_sources(&app.db(), &cfgs).unwrap();
    app.platform
        .secret_set(crate::sync::connectors::obsidian::rest::API_KEY, "k3y")
        .unwrap();
    (app, dir)
}

fn note_json(content: &str) -> String {
    json!({ "path": "p.md", "content": content, "stat": { "ctime": 1, "mtime": 2, "size": 3 },
            "tags": [], "frontmatter": {}, "links": [], "backlinks": [], "unresolvedLinks": [] })
    .to_string()
}

#[test]
fn sample_data_writes_directly() {
    let dir = tempfile::tempdir().unwrap();
    let app = App::from_parts(crate::db::open_in_memory().unwrap(), Settings::default(), dir.path()).unwrap();
    assert!(writer(&app).unwrap().is_none(), "the sample Obsidian row has no vault");
    let rel = "Topics/Q4 headcount.md";
    let before = disk_text(&app, rel).unwrap();
    assert!(matches!(save_note(&app, rel, Some("stale"), "x").unwrap(), Saved::Conflict(Some(ref d)) if *d == before));
    assert!(matches!(
        save_note(&app, rel, Some(&before), "mine\n").unwrap(),
        Saved::Written
    ));
    assert_eq!(disk_text(&app, rel).unwrap(), "mine\n");
    assert!(save_note(&app, "../x.md", None, "x").is_err());
}

#[test]
fn queued_saves_follow_the_outbox() {
    let (app, _d) = app_with_source("obsidian");
    let rel = "Topics/Q4 headcount.md";
    let base = disk_text(&app, rel).unwrap();
    let Saved::Queued(id) = save_note(&app, rel, Some(&base), "v1\n").unwrap() else {
        panic!()
    };
    assert_eq!(status(&app, id).unwrap()["state"], "saving");
    // More typing before it's sent: same row, newest text, original base.
    let Saved::Queued(id2) = save_note(&app, rel, Some("v1\n"), "v2\n").unwrap() else {
        panic!()
    };
    assert_eq!(id2, id);

    // Obsidian is closed: the row fails with the "isn't open" message (the
    // mapping from a refused connection is tested in rest.rs).
    let http = FakeHttp::default();
    app.db()
        .execute(
            "UPDATE outbox SET state = 'failed', error = ?2 WHERE id = ?1",
            params![id, NOT_OPEN],
        )
        .unwrap();
    assert_eq!(status(&app, id).unwrap()["state"], "offline");
    // Still typing while it's closed: folded into the failed row.
    let Saved::Queued(id3) = save_note(&app, rel, Some("v2\n"), "v3\n").unwrap() else {
        panic!()
    };
    assert_eq!(id3, id);

    // ⌘R, Obsidian open, note unchanged: it lands through the API.
    app.db()
        .execute("UPDATE outbox SET state = 'pending' WHERE id = ?1", [id])
        .unwrap();
    http.on("GET", "/vault/Topics/Q4%20headcount.md", 200, note_json(&base));
    http.on("PUT", "/vault/Topics/Q4%20headcount.md", 204, "");
    assert!(crate::outbox::process_one(&app, &http).unwrap());
    assert_eq!(status(&app, id).unwrap()["state"], "saved");
    let put: Vec<_> = http.requests().into_iter().filter(|r| r.0 == "PUT").collect();
    assert_eq!(put[0].2.as_deref(), Some("v3\n"));

    // The next save starts a new row from the saved text.
    let Saved::Queued(id4) = save_note(&app, rel, Some("v3\n"), "v4\n").unwrap() else {
        panic!()
    };
    assert_ne!(id4, id);
}

#[test]
fn conflicts_then_keep_mine() {
    let (app, _d) = app_with_source("obsidian");
    let rel = "Topics/Q4 headcount.md";
    let base = disk_text(&app, rel).unwrap();
    let Saved::Queued(id) = save_note(&app, rel, Some(&base), "mine\n").unwrap() else {
        panic!()
    };
    // Someone edited the note in Obsidian (and it saved to disk).
    std::fs::write(app.vault.join(rel), "theirs\n").unwrap();
    let http = FakeHttp::default();
    http.on("GET", "/vault/", 200, note_json("theirs\n"));
    http.on("PUT", "/vault/", 204, "");
    crate::outbox::process_one(&app, &http).unwrap();
    let s = status(&app, id).unwrap();
    assert_eq!(
        (s["state"].as_str(), s["disk"].as_str()),
        (Some("conflict"), Some("theirs\n"))
    );
    assert!(http.requests().iter().all(|r| r.0 != "PUT"));

    keep_mine(&app, id).unwrap();
    crate::outbox::process_one(&app, &http).unwrap();
    assert_eq!(status(&app, id).unwrap()["state"], "saved");
    assert!(keep_mine(&app, id).is_err());
}

#[test]
fn file_mode_goes_through_the_outbox_too() {
    let (app, _d) = app_with_source("file");
    let rel = "Topics/Alerting P2.md";
    let base = disk_text(&app, rel).unwrap();
    let Saved::Queued(id) = save_note(&app, rel, Some(&base), "edited\n").unwrap() else {
        panic!()
    };
    crate::outbox::process_one(&app, &FakeHttp::default()).unwrap();
    assert_eq!(status(&app, id).unwrap()["state"], "saved");
    assert_eq!(disk_text(&app, rel).unwrap(), "edited\n");
}
