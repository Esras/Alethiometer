use super::*;
use crate::queries::people::resolver;

const KIM: &str = "---\nslack: U0KIM\nemail: [kim@work.com]\nrole: Buyer\nteam: \"[[Procurement]]\"\n---\n## How Kim works\n- Prefers a short doc over a call\n";
const KIMBERLY: &str = "---\naliases:\n  - Kimberly\nemail: [kimberly.p@work.com, Kim@work.com]\nrole: Procurement analyst\nmanager: Dana\n---\n- Out until Sep 1\n";

fn take(keys: &[&str]) -> HashSet<String> {
    keys.iter().map(|k| k.to_string()).collect()
}

#[test]
fn lists_union_missing_fields_copy_and_a_differing_field_is_a_conflict() {
    assert_eq!(
        conflicts(KIM, KIMBERLY),
        [Conflict {
            key: "role".into(),
            keep: vec!["Buyer".into()],
            gone: vec!["Procurement analyst".into()]
        }],
        "accounts and aliases never conflict; a field only one has doesn't either"
    );
    assert_eq!(
        merged(KIM, KIMBERLY, "Kimberly P.", &take(&[])),
        "---\nslack: U0KIM\nemail: [kim@work.com, kimberly.p@work.com]\nrole: Buyer\nteam: \"[[Procurement]]\"\naliases: Kimberly\nmanager: Dana\n---\n## How Kim works\n- Prefers a short doc over a call\n\n## Merged from Kimberly P.\n- Out until Sep 1\n"
    );
    assert!(
        merged(KIM, KIMBERLY, "Kimberly P.", &take(&["role"])).contains("\nrole: Procurement analyst\n"),
        "the other side, when taken"
    );
}

#[test]
fn an_empty_body_appends_nothing_and_crlf_stays_crlf() {
    let keep = "---\r\nslack: U1\r\n---\r\nNotes\r\n";
    assert_eq!(
        merged(keep, "---\nslack: U2\n---\n", "B", &take(&[])),
        "---\r\nslack: [U1, U2]\r\n---\r\nNotes\r\n"
    );
    assert_eq!(
        merged(keep, "line one\nline two\n", "B", &take(&[])),
        "---\r\nslack: U1\r\n---\r\nNotes\r\n\r\n## Merged from B\r\nline one\r\nline two\r\n",
        "a note without frontmatter is all body"
    );
}

#[test]
fn set_field_replaces_scalars_and_block_lists() {
    let t = "---\nrole: A\nteam:\n  - X\n  - Y\nmore: 1\n---\nBody\n";
    assert_eq!(
        set_field(t, "team", &["Z".into()]),
        "---\nrole: A\nteam: Z\nmore: 1\n---\nBody\n"
    );
    assert_eq!(
        set_field(t, "role", &["B".into(), "C: d".into()]),
        "---\nrole: [B, \"C: d\"]\nteam:\n  - X\n  - Y\nmore: 1\n---\nBody\n"
    );
    assert_eq!(
        set_field(t, "new", &["v".into()]),
        "---\nrole: A\nteam:\n  - X\n  - Y\nmore: 1\nnew: v\n---\nBody\n"
    );
    assert_eq!(set_field("Body\n", "role", &["A".into()]), "---\nrole: A\n---\nBody\n");
}

#[test]
fn merging_without_obsidian_writes_trashes_and_moves_topic_links() {
    let (app, _dir) = crate::ai::context::tests::app();
    let people = app.vault.join("People");
    std::fs::create_dir_all(&people).unwrap();
    std::fs::write(people.join("Kim Test.md"), KIM).unwrap();
    std::fs::write(people.join("Kimberly P.md"), KIMBERLY).unwrap();
    rescan(&app).unwrap();
    let r = resolver(&app).unwrap();
    let keep = r.who("slack", Some("U0KIM"), "").person_id.unwrap();
    let gone = r.who("gmail", Some("kimberly.p@work.com"), "").person_id.unwrap();
    app.db()
        .execute(
            "INSERT INTO topic_people (topic_id, person_id) SELECT id, ?1 FROM topics LIMIT 1",
            [gone],
        )
        .unwrap();
    let p = preview(&app, &json!({ "keep": keep, "gone": gone, "take": ["role"] })).unwrap();
    assert_eq!(p["conflicts"][0]["key"], "role");
    assert!(p["merged"].as_str().unwrap().contains("role: Procurement analyst"));
    assert_eq!(p["gone"]["path"], "People/Kimberly P.md");

    let args = |gone_text: &str| json!({ "keep": keep, "gone": gone, "take": ["role"], "keep_text": KIM, "gone_text": gone_text });

    // The other note changed after the preview: nothing is written.
    let edited = format!("{KIMBERLY}- edited\n");
    std::fs::write(people.join("Kimberly P.md"), &edited).unwrap();
    assert!(merge(&app, &args(KIMBERLY)).is_err());
    assert_eq!(std::fs::read_to_string(people.join("Kim Test.md")).unwrap(), KIM);
    assert!(people.join("Kimberly P.md").exists());

    let v = merge(&app, &args(&edited)).unwrap();
    assert_eq!(v["state"], "written");
    let text = std::fs::read_to_string(people.join("Kim Test.md")).unwrap();
    assert!(
        text.contains("kimberly.p@work.com") && text.ends_with("- Out until Sep 1\n- edited\n"),
        "{text}"
    );
    assert!(!people.join("Kimberly P.md").exists());
    assert!(app.vault.join(".trash/Kimberly P.md").exists());
    let r = resolver(&app).unwrap();
    assert_eq!(r.who("gmail", Some("kimberly.p@work.com"), "").person_id, Some(keep));
    let linked: Vec<i64> = app
        .db()
        .prepare("SELECT person_id FROM topic_people")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    assert!(linked.contains(&keep) && !linked.contains(&gone), "{linked:?}");
}

#[test]
fn a_person_cant_be_merged_into_themselves() {
    let (app, _dir) = crate::ai::context::tests::app();
    assert!(preview(&app, &json!({ "keep": 1, "gone": 1 })).is_err());
}

#[test]
fn a_queued_merge_moves_topic_links_when_obsidian_confirms_it() {
    let conn = crate::db::open_in_memory().unwrap();
    conn.execute_batch(
        "INSERT INTO people (id, display_name) VALUES (1, 'Kim'), (2, 'Kimberly P.');
         INSERT INTO topics (id, name, category) VALUES (1, 'A', 'work'), (2, 'B', 'work');
         INSERT INTO topic_people (topic_id, person_id) VALUES (1, 1), (1, 2), (2, 2);",
    )
    .unwrap();
    crate::outbox::tests_on_confirmed(
        &conn,
        "obsidian.merge_notes",
        &json!({ "people": { "keep": 1, "gone": 2 } }),
        &json!({}),
    );
    let links: Vec<(i64, i64)> = conn
        .prepare("SELECT topic_id, person_id FROM topic_people ORDER BY 1, 2")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    assert_eq!(links, [(1, 1), (2, 1)]);
}
