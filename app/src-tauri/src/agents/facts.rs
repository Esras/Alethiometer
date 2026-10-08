//! The pane header's facts box: ticket, PR, branch and todo for a session.
//! The branch comes from git in the session's directory (or the Scion agent's
//! branch); the ticket is a Jira key in the branch, title or first prompt; the
//! PR is a GitHub item for that branch or key; the todo is one naming the key.

use super::model::{Fact, Facts};
use crate::domain::ports::Reads;
use crate::error::Result;
use crate::state::App;
use std::path::Path;
use std::process::Command;

/// The repo a directory belongs to (its git top level's name), else the directory's name.
pub fn project_of(cwd: &str) -> String {
    let top = git(cwd, &["rev-parse", "--show-toplevel"]).unwrap_or_else(|| cwd.to_owned());
    Path::new(top.trim_end_matches('/'))
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn git(cwd: &str, args: &[&str]) -> Option<String> {
    if cwd.is_empty() {
        return None;
    }
    let out = Command::new("git").arg("-C").arg(cwd).args(args).output().ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_owned())
        .filter(|s| !s.is_empty())
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
    let name = branch
        .map(str::to_owned)
        .or_else(|| git(cwd, &["rev-parse", "--abbrev-ref", "HEAD"]))?;
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
    Some(Fact {
        label: name,
        detail,
        url: String::new(),
        done: false,
    })
}

/// Ticket, PR and todo from the local database.
pub fn from_db(
    r: &dyn Reads,
    clock: &crate::clock::Clock,
    key: Option<&str>,
    branch: Option<&str>,
) -> Result<(Option<Fact>, Option<Fact>, Option<Fact>)> {
    let ticket = match key {
        Some(k) => Some(match r.agents().ticket(k)? {
            Some(t) => Fact {
                label: k.to_owned(),
                detail: t
                    .summary
                    .unwrap_or_default()
                    .split(" · ")
                    .next()
                    .unwrap_or("")
                    .to_owned(),
                url: t.url.unwrap_or_default(),
                done: t.done,
            },
            None => Fact {
                label: k.to_owned(),
                ..Default::default()
            },
        }),
        None => None,
    };
    let pr = r.agents().pr(branch, key)?.map(|p| {
        let url = p.url.unwrap_or_default();
        let n = url.rsplit('/').next().unwrap_or("").to_owned();
        let detail = if p.merged {
            "merged"
        } else if p.status == "done" {
            "closed"
        } else if p.draft {
            "draft"
        } else {
            ""
        };
        Fact {
            label: format!("#{n}"),
            detail: detail.into(),
            url,
            done: p.merged,
        }
    });
    // The open task for this ticket due soonest (by its real deadline here; undated last).
    let todo = match key {
        Some(k) => r
            .agents()
            .open_tasks_for(k)?
            .into_iter()
            .min_by_key(|t| (t.due.is_none(), t.due.map(|d| d.deadline(clock))))
            .map(|t| {
                let from = match t.source_kind.as_str() {
                    "notes" => "Local",
                    "jira" => "Jira",
                    "obsidian" => "Obsidian",
                    other => other,
                };
                let due = t
                    .due
                    .map(|d| clock.zoned(d.deadline(clock)).strftime("%a").to_string())
                    .unwrap_or_default();
                Fact {
                    label: format!("{from} · {}", t.title),
                    detail: due,
                    url: t.url.unwrap_or_default(),
                    done: false,
                }
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
        let Some(s) = app.agents.store.lock().sessions.get(&id).cloned() else {
            return;
        };
        let first_prompt = app
            .agents
            .store
            .lock()
            .find_row(&super::session_thread(&id), |r| r.kind == super::model::Kind::You)
            .map(|r| r.text.clone())
            .unwrap_or_default();
        let scion_branch = s
            .facts
            .branch
            .as_ref()
            .filter(|_| s.source == "scion")
            .map(|b| b.label.clone());
        let branch = branch_fact(&s.cwd, scion_branch.as_deref()).or(s.facts.branch.clone());
        let key = ticket_key(
            branch.as_ref().map_or("", |b| b.label.as_str()),
            &[&s.title, &first_prompt],
        );
        let Ok((ticket, pr, todo)) = app
            .store()
            .read(|r| from_db(r, &app.clock, key.as_deref(), branch.as_ref().map(|b| b.label.as_str())))
        else {
            return;
        };
        let facts = Facts {
            ticket,
            pr,
            branch,
            todo,
        };
        app.agents.store.lock().update_session(&id, |s| s.facts = facts);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_ticket_keys_in_branches_and_text() {
        assert_eq!(ticket_key("fix/ale-92-settings-header", &[]).as_deref(), Some("ALE-92"));
        assert_eq!(
            ticket_key("main", &["Replace the poller (ING-31)"]).as_deref(),
            Some("ING-31")
        );
        assert_eq!(ticket_key("main", &["utf-8 fixes"]), None);
    }

    #[test]
    fn ticket_pr_and_todo_from_the_database() {
        let c = crate::db::open_in_memory().unwrap();
        c.execute_batch(
            "INSERT INTO sources (id, kind, tag, name) VALUES (1, 'jira', 'work', 'jira'), (2, 'github', 'work', 'gh'), (3, 'notes', 'work', 'notes');
             INSERT INTO items (source_id, external_id, kind, title, summary, url, occurred_at) VALUES
               (1, 'ING-31', 'issue', 'Webhooks', 'In progress · you', 'https://j/ING-31', '2026-10-01T10:00:00.000Z');
             INSERT INTO items (source_id, external_id, kind, title, url, occurred_at, raw_json) VALUES
               (2, 'pr-212', 'pr', 'Jira webhooks', 'https://github.com/o/r/pull/212', '2026-10-02T10:00:00.000Z', '{\"draft\":true,\"head\":{\"ref\":\"feat/jira-webhooks\"}}');
             INSERT INTO items (source_id, external_id, kind, title, occurred_at, due_at) VALUES
               (3, 'l1', 'task', 'ING-31 webhooks cutover', '2026-10-01T10:00:00.000Z', '2026-10-08T00:00:00.000Z');",
        )
        .unwrap();
        let (t, p, d) = from_db(
            &crate::store::over(&c),
            &crate::sync::test_clock("2026-09-28T10:52:00Z"),
            Some("ING-31"),
            Some("feat/jira-webhooks"),
        )
        .unwrap();
        let t = t.unwrap();
        assert_eq!((t.label.as_str(), t.detail.as_str()), ("ING-31", "In progress"));
        let p = p.unwrap();
        assert_eq!((p.label.as_str(), p.detail.as_str()), ("#212", "draft"));
        let d = d.unwrap();
        assert_eq!(d.label, "Local · ING-31 webhooks cutover");
        assert_eq!(d.detail, "Thu");
    }
}
