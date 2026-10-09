//! Rules about where code lives.

use std::path::{Path, PathBuf};

/// The `src/` of every crate in the app.
fn crate_sources() -> Vec<PathBuf> {
    let app = Path::new(env!("CARGO_MANIFEST_DIR"));
    ["src", "../core/src", "../settings/src", "../platform/src"]
        .iter()
        .map(|d| app.join(d))
        .collect()
}

/// SQL lives in `core/src/store/` (tests' own fixtures aside).
#[test]
fn no_sql_outside_store() {
    let re = regex::Regex::new(r#"(?i)"\s*(SELECT|INSERT\s+(OR\s+\w+\s+)?INTO|UPDATE\s+\w+\s+SET|DELETE\s+FROM|CREATE\s+(TABLE|INDEX|TRIGGER|VIEW)|ALTER\s+TABLE|DROP\s+TABLE|WITH\s+\w+\s+AS)\b"#).unwrap();
    let test_mod = regex::Regex::new(r"#\[cfg\(test\)\]\s*(pub(\(crate\))?\s+)?mod\s+\w+").unwrap();
    let app = Path::new(env!("CARGO_MANIFEST_DIR"));
    let store = app.join("../core/src/store");
    let mut found = vec![];
    for root in crate_sources() {
        let mut dirs = vec![root.clone()];
        while let Some(dir) = dirs.pop() {
            for e in std::fs::read_dir(dir).unwrap().flatten() {
                let p = e.path();
                if p.is_dir() {
                    if p != store {
                        dirs.push(p);
                    }
                } else if p.extension().is_some_and(|x| x == "rs") && !p.ends_with("tests.rs") {
                    let text = std::fs::read_to_string(&p).unwrap();
                    // Test fixtures may write rows directly: a file's test modules are left out.
                    let code = test_mod.split(&text).next().unwrap();
                    let n = re.find_iter(code).count();
                    if n > 0 {
                        found.push(format!(
                            "{}: {n} SQL statements (move them into a repository in store/)",
                            p.strip_prefix(&root).unwrap().display()
                        ));
                    }
                }
            }
        }
    }
    assert!(found.is_empty(), "{}", found.join("\n"));
}

/// Wall-clock time comes from `Clock` only. Protoncal hands its zone to
/// `rrule` as chrono values, converted from the clock's; this file and the
/// UTC migration name the old forms in prose.
#[test]
fn nothing_else_reads_local_time() {
    let banned = [
        "chrono::Local",
        "Local::now",
        "TimeZone::system",
        "Zoned::now",
        "localtime",
        "datetime('now",
        "date('now",
        "CURRENT_TIMESTAMP",
    ];
    let allowed = ["clock.rs", "utc_migration.rs", "utc_migration/tests.rs", "protoncal.rs"];
    let mut bad = vec![];
    let mut dirs = crate_sources();
    while let Some(dir) = dirs.pop() {
        for e in std::fs::read_dir(dir).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                dirs.push(p);
            } else if p.extension().is_some_and(|x| x == "rs") && !allowed.iter().any(|a| p.ends_with(a)) {
                let text = std::fs::read_to_string(&p).unwrap();
                bad.extend(
                    banned
                        .iter()
                        .filter(|b| text.contains(*b))
                        .map(|b| format!("{}: {b}", p.display())),
                );
            }
        }
    }
    assert!(bad.is_empty(), "local time read outside Clock: {bad:#?}");
    // Column defaults in the migrated schema.
    let conn = alethiometer_core::store::schema::open_in_memory().unwrap();
    let schema: Vec<String> = conn
        .prepare("SELECT sql FROM sqlite_master WHERE sql IS NOT NULL")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    let bad: Vec<&String> = schema.iter().filter(|s| banned.iter().any(|b| s.contains(b))).collect();
    assert!(bad.is_empty(), "defaults that aren't UTC instants: {bad:#?}");
}
