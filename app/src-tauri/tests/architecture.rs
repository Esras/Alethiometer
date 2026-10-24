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

/// Production code of a source file: test modules are left out.
fn production_code(text: &str) -> &str {
    let test_mod = regex::Regex::new(r"#\[cfg\(test\)\]\s*(pub(\(crate\))?\s+)?mod\s+\w+").unwrap();
    let end = test_mod.find(text).map_or(text.len(), |m| m.start());
    &text[..end]
}

/// Rust files under `src/` whose path (relative, `/`-separated) starts with one of `scopes`, with `count` matches each.
fn count_in(
    scopes: &[&str],
    skip: &[&str],
    count: impl Fn(&str) -> usize,
) -> std::collections::BTreeMap<String, usize> {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut found = std::collections::BTreeMap::new();
    let mut dirs = vec![src.clone()];
    while let Some(dir) = dirs.pop() {
        for e in std::fs::read_dir(dir).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                dirs.push(p);
                continue;
            }
            let rel = p.strip_prefix(&src).unwrap().to_string_lossy().replace('\\', "/");
            let wanted = scopes.iter().any(|s| rel.starts_with(s)) && !skip.contains(&rel.as_str());
            if wanted && rel.ends_with(".rs") && !rel.ends_with("tests.rs") {
                let n = count(production_code(&std::fs::read_to_string(&p).unwrap()));
                if n > 0 {
                    found.insert(rel, n);
                }
            }
        }
    }
    found
}

/// The allowlist is exact: a new violation fails, and so does a fixed one nobody removed from the list.
fn assert_allowlist(rule: &str, found: std::collections::BTreeMap<String, usize>, allowed: &[(&str, usize)]) {
    let allowed: std::collections::BTreeMap<String, usize> = allowed.iter().map(|(f, n)| (f.to_string(), *n)).collect();
    if found == allowed {
        return;
    }
    let mut msg = vec![format!("{rule}")];
    for f in found
        .keys()
        .chain(allowed.keys())
        .collect::<std::collections::BTreeSet<_>>()
    {
        let (now, then) = (found.get(f).copied().unwrap_or(0), allowed.get(f).copied().unwrap_or(0));
        if now > then {
            msg.push(format!("  {f}: {now}, allowed {then}: new violation"));
        } else if now < then {
            msg.push(format!("  {f}: {now}, allowed {then}: fixed, lower the allowlist"));
        }
    }
    msg.push("current counts:".into());
    msg.extend(found.iter().map(|(f, n)| format!("    (\"{f}\", {n}),")));
    panic!("{}", msg.join("\n"));
}

/// A function that takes the whole `App` can reach everything. New code takes the ports, settings and
/// secrets it uses. `api`, `state` and the entry points are where `App` is wired, so they are exempt.
#[test]
fn services_do_not_take_app() {
    let sig = regex::Regex::new(r"(?s)\bfn\s+\w+[^{;]*").unwrap();
    let app = regex::Regex::new(r"\bApp\b").unwrap();
    let found = count_in(&[""], &["api.rs", "state.rs", "lib.rs", "main.rs"], |code| {
        sig.find_iter(code).filter(|m| app.is_match(m.as_str())).count()
    });
    let found = found.into_iter().filter(|(f, _)| !f.starts_with("bin/")).collect();
    assert_allowlist(
        "functions with `App` in their signature",
        found,
        &[
            ("agents/acp.rs", 40),
            ("agents/archive.rs", 5),
            ("agents/daemon.rs", 12),
            ("agents/explain.rs", 3),
            ("agents/facts.rs", 1),
            ("agents/guess.rs", 1),
            ("agents/incoming.rs", 3),
            ("agents/mod.rs", 9),
            ("agents/openshell.rs", 7),
            ("agents/scion.rs", 25),
            ("ai/context.rs", 3),
            ("ai/estimate.rs", 1),
            ("ai/mod.rs", 4),
            ("ai/suggest.rs", 2),
            ("embed/mod.rs", 5),
            ("embed/model.rs", 3),
            ("glean/ask.rs", 5),
            ("glean/hydrate.rs", 4),
            ("glean/mod.rs", 3),
            ("live.rs", 11),
            ("outbox.rs", 3),
            ("queries/categories.rs", 2),
            // `merge_notes` (People merge) added 2026-10-24.
            ("queries/note_save.rs", 8),
            ("queries/notes.rs", 5),
            ("queries/people.rs", 7),
            // People merge (`preview`, `merge`, and their `sides`), 2026-10-24: they read notes in the vault.
            ("queries/person_merge.rs", 3),
            ("queries/reflect.rs", 8),
            ("queries/search.rs", 1),
            ("queries/topics.rs", 5),
            ("seed.rs", 1),
            ("settings/api.rs", 4),
            // The settings writer; `mute` (ignore suggestions) and `patch_with` added 2026-10-17.
            ("settings/live.rs", 11),
            ("standin.rs", 3),
            ("sync/connectors/gcal/oauth.rs", 4),
            ("sync/connectors/obsidian.rs", 1),
            ("sync/connectors/slack/socket.rs", 3),
            ("sync/connectors/teams/oauth.rs", 4),
            ("sync/mod.rs", 6),
            ("sync/worker.rs", 8),
        ],
    );
}

fn imports(code: &str, modules: &str) -> usize {
    let direct = regex::Regex::new(&format!(r"crate::({modules})\b")).unwrap();
    let grouped = regex::Regex::new(&format!(r"use crate::\{{[^}}]*\b({modules})\b")).unwrap();
    direct.find_iter(code).count() + grouped.find_iter(code).count()
}

/// A connector talks to its source through `sync::Ctx`: not to the services above it, not to `App`.
#[test]
fn connectors_do_not_reach_into_services() {
    let found = count_in(&["sync/connectors/"], &[], |code| {
        imports(code, "queries|state|agents|ai")
    });
    assert_allowlist(
        "connector imports of queries, state, agents or ai",
        found,
        &[
            ("sync/connectors/agents.rs", 2),
            ("sync/connectors/gcal/oauth.rs", 1),
            ("sync/connectors/obsidian.rs", 1),
            ("sync/connectors/scion.rs", 1),
            ("sync/connectors/slack/socket.rs", 1),
            ("sync/connectors/teams/oauth.rs", 1),
        ],
    );
}

/// Queries are the app's own use cases; they don't call the machinery that feeds them.
#[test]
fn queries_do_not_call_sync_ai_or_agents() {
    let found = count_in(&["queries/"], &[], |code| imports(code, "sync|ai|agents"));
    assert_allowlist("queries imports of sync, ai or agents", found, &[]);
}
