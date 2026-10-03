//! The pane header's facts box: ticket, PR, branch and todo for a session.
//! The branch comes from git in the session's directory (or the Scion agent's
//! branch); the ticket is a Jira key in the branch, title or first prompt; the
//! PR is a GitHub item for that branch or key; the todo is one naming the key.

use super::model::{Fact, Facts};
use crate::state::App;
use rusqlite::{params, Connection, OptionalExtension};
use std::path::Path;
use std::process::Command;

/// The repo a directory belongs to (its git top level's name), else the directory's name.
pub fn project_of(cwd: &str) -> String {
    let top = git(cwd, &["rev-parse", "--show-toplevel"]).unwrap_or_else(|| cwd.to_owned());
    Path::new(top.trim_end_matches('/')).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

fn git(cwd: &str, args: &[&str]) -> Option<String> {
    if cwd.is_empty() {
        return None;
    }
    let out = Command::new("git").arg("-C").arg(cwd).args(args).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_owned()).filter(|s| !s.is_empty())
}

/// The first Jira-style key (`ALE-92`) in the branch (any case: fix/ale-92-header)
/// or in `texts` (upper case only, so "utf-8" in prose isn't one).
pub fn ticket_key(branch: &str, texts: &[&str]) -> Option<String> {
    let re = regex::Regex::new(r"\b([A-Z][A-Z0-9]{1,9}-\d{1,6})\b").ok()?;
    let upper = branch.to_uppercase();
    re.captures(&upper)
        .map(|c| c[1].to_owned())
        .or_else(|| texts.iter().find_map(|t| re.captures(t).map(|c| c[1].to_owned())))
}

/// Branch and how far ahead of its upstream (or origin's default) it is.
pub fn branch_fact(cwd: &str, branch: Option<&str>) -> Option<Fact> {
    let name = branch.map(str::to_owned).or_else(|| git(cwd, &["rev-parse", "--abbrev-ref", "HEAD"]))?;
    if name == "HEAD" {
        return None;
    }
    let ahead = git(cwd, &["rev-list", "--count", "@{upstream}..HEAD"])
        .or_else(|| git(cwd, &["rev-list", "--count", "origin/HEAD..HEAD"]))
        .and_then(|n| n.parse::<u64>().ok());
    let detail = match ahead {
        Some(0) | None => String::new(),
        Some(n) => format!("{n} ahead"),
    };
    Some(Fact { label: name, detail, url: String::new() })
}

/// Ticket, PR and todo from the local database.
pub fn from_db(conn: &Connection, key: Option<&str>, branch: Option<&str>) -> rusqlite::Result<(Option<Fact>, Option<Fact>, Option<Fact>)> {
    let ticket = match key {
        Some(k) => conn
            .query_row(
                "SELECT i.summary, i.url FROM items i JOIN sources s ON s.id = i.source_id WHERE s.kind = 'jira' AND i.external_id = ?1",
                [k],
                |r| Ok((r.get::<_, Option<String>>(0)?, r.get::<_, Option<String>>(1)?)),
            )
            .optional()?
            .map(|(summary, url)| Fact {
                label: k.to_owned(),
                detail: summary.unwrap_or_default().split(" · ").next().unwrap_or("").to_owned(),
                url: url.unwrap_or_default(),
            })
            .or_else(|| Some(Fact { label: k.to_owned(), ..Default::default() })),
        None => None,
    };
    let pr = conn
        .query_row(
            "SELECT i.url, i.status, json_extract(i.raw_json, '$.draft') FROM items i JOIN sources s ON s.id = i.source_id
             WHERE s.kind = 'github' AND i.kind = 'pr'
               AND ((?1 IS NOT NULL AND json_extract(i.raw_json, '$.head.ref') = ?1) OR (?2 IS NOT NULL AND instr(upper(i.title), ?2) > 0))
             ORDER BY i.occurred_at DESC LIMIT 1",
            params![branch, key],
            |r| Ok((r.get::<_, Option<String>>(0)?, r.get::<_, String>(1)?, r.get::<_, Option<i64>>(2)?)),
        )
        .optional()?
        .map(|(url, status, draft)| {
            let url = url.unwrap_or_default();
            let n = url.rsplit('/').next().unwrap_or("").to_owned();
            let detail = if draft == Some(1) { "draft".to_owned() } else if status == "done" { "merged".to_owned() } else { String::new() };
            Fact { label: format!("#{n}"), detail, url }
        });
    let todo = match key {
        Some(k) => conn
            .query_row(
                "SELECT i.title, s.kind, i.due_at, i.url FROM items i JOIN sources s ON s.id = i.source_id
                 WHERE i.kind = 'task' AND i.status != 'done' AND (i.external_id = ?1 OR instr(upper(i.title), ?1) > 0)
                 ORDER BY i.due_at IS NULL, i.due_at LIMIT 1",
                [k],
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, Option<String>>(2)?, r.get::<_, Option<String>>(3)?)),
            )
            .optional()?
            .map(|(title, kind, due, url)| {
                let from = match kind.as_str() {
                    "notes" => "Local",
                    "jira" => "Jira",
                    "obsidian" => "Obsidian",
                    other => other,
                };
                let due = due
                    .and_then(|d| crate::queries::parse_ts(&d))
                    .map(|d| d.format("%a").to_string())
                    .unwrap_or_default();
                Fact { label: format!("{from} · {title}"), detail: due, url: url.unwrap_or_default() }
            }),
        None => None,
    };
    Ok((ticket, pr, todo))
}

/// Recompute a session's facts in the background (git can be slow on a big repo).
pub fn refresh(app: &App, id: &str) {
    let Some(app) = app.arc() else { return };
    let id = id.to_owned();
    std::thread::spawn(move || {
        let Some(s) = app.agents.store.lock().sessions.get(&id).cloned() else { return };
        let first_prompt = app
            .agents
            .store
            .lock()
            .find_row(&super::session_thread(&id), |r| r.kind == super::model::Kind::You)
            .map(|r| r.text.clone())
            .unwrap_or_default();
        let scion_branch = s.facts.branch.as_ref().filter(|_| s.source == "scion").map(|b| b.label.clone());
        let branch = branch_fact(&s.cwd, scion_branch.as_deref()).or(s.facts.branch.clone());
        let key = ticket_key(branch.as_ref().map_or("", |b| b.label.as_str()), &[&s.title, &first_prompt]);
        let Ok((ticket, pr, todo)) = from_db(&app.db(), key.as_deref(), branch.as_ref().map(|b| b.label.as_str())) else { return };
        let facts = Facts { ticket, pr, branch, todo };
        app.agents.store.lock().update_session(&id, |s| s.facts = facts);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_ticket_keys_in_branches_and_text() {
        assert_eq!(ticket_key("fix/ale-92-settings-header", &[]).as_deref(), Some("ALE-92"));
        assert_eq!(ticket_key("main", &["Replace the poller (ING-31)"]).as_deref(), Some("ING-31"));
        assert_eq!(ticket_key("main", &["utf-8 fixes"]), None);
    }

    #[test]
    fn ticket_pr_and_todo_from_the_database() {
        let c = crate::db::open_in_memory().unwrap();
        c.execute_batch(
            "INSERT INTO sources (id, kind, tag, name) VALUES (1, 'jira', 'work', 'jira'), (2, 'github', 'work', 'gh'), (3, 'notes', 'work', 'notes');
             INSERT INTO items (source_id, external_id, kind, title, summary, url, occurred_at) VALUES
               (1, 'ING-31', 'issue', 'Webhooks', 'In progress · you', 'https://j/ING-31', '2026-10-01T10:00:00');
             INSERT INTO items (source_id, external_id, kind, title, url, occurred_at, raw_json) VALUES
               (2, 'pr-212', 'pr', 'Jira webhooks', 'https://github.com/o/r/pull/212', '2026-10-02T10:00:00', '{\"draft\":true,\"head\":{\"ref\":\"feat/jira-webhooks\"}}');
             INSERT INTO items (source_id, external_id, kind, title, occurred_at, due_at) VALUES
               (3, 'l1', 'task', 'ING-31 webhooks cutover', '2026-10-01T10:00:00', '2026-10-08T00:00:00');",
        )
        .unwrap();
        let (t, p, d) = from_db(&c, Some("ING-31"), Some("feat/jira-webhooks")).unwrap();
        let t = t.unwrap();
        assert_eq!((t.label.as_str(), t.detail.as_str()), ("ING-31", "In progress"));
        let p = p.unwrap();
        assert_eq!((p.label.as_str(), p.detail.as_str()), ("#212", "draft"));
        let d = d.unwrap();
        assert_eq!(d.label, "Local · ING-31 webhooks cutover");
        assert_eq!(d.detail, "Thu");
    }
}
