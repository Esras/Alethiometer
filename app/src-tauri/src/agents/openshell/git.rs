use crate::error::{msg, Result};
use serde::Serialize;
use std::path::Path;
use std::process::{Command, Stdio};

pub fn local(dir: &str, args: &[&str]) -> Result<String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| msg(format!("git: {e}")))?;
    if out.status.success() {
        return Ok(String::from_utf8_lossy(&out.stdout).trim_end().to_owned());
    }
    let err = String::from_utf8_lossy(&out.stderr);
    Err(msg(format!(
        "git {}: {}",
        args.first().unwrap_or(&""),
        err.lines().rfind(|l| !l.trim().is_empty()).unwrap_or("failed")
    )))
}

/// A subfolder of a repository is uploaded as plain files.
pub fn is_top(dir: &str) -> bool {
    let canon = |p: &str| std::fs::canonicalize(p).ok();
    local(dir, &["rev-parse", "--show-toplevel"]).is_ok_and(|t| canon(&t).is_some() && canon(&t) == canon(dir))
}

pub fn current_branch(dir: &str) -> Option<String> {
    local(dir, &["symbolic-ref", "--short", "-q", "HEAD"])
        .ok()
        .filter(|b| !b.is_empty())
}

/// Returns the branch's tip, the base changes come back against.
pub fn bundle(dir: &str, branch: &str, out: &Path) -> Result<String> {
    let base = local(
        dir,
        &["rev-parse", "--verify", &format!("refs/heads/{branch}^{{commit}}")],
    )
    .map_err(|_| msg(format!("{dir} has no branch {branch}")))?;
    let out = out.display().to_string();
    local(dir, &["bundle", "create", "-q", &out, &format!("refs/heads/{branch}")])?;
    Ok(base)
}

fn q(s: &str) -> String {
    shlex::try_quote(s).map(|c| c.into_owned()).unwrap_or_default()
}

pub fn clone_script(bundle: &str, dest: &str, branch: &str, name: &str, email: &str) -> String {
    let mut s = format!(
        "set -e; git clone -q --branch {b} -- {bundle} {dest}; cd {dest}; git remote remove origin; rm -f {bundle}",
        b = q(branch),
        bundle = q(bundle),
        dest = q(dest),
    );
    if !name.is_empty() {
        s.push_str(&format!("; git config user.name {}", q(name)));
    }
    if !email.is_empty() {
        s.push_str(&format!("; git config user.email {}", q(email)));
    }
    s
}

/// Uncommitted work becomes a commit made from a scratch index, so the agent's own index and files stay as they are.
pub fn back_script(dir: &str, base: &str, out: &str) -> String {
    format!(
        r#"set -e; cd {dir}
head=$(git rev-parse HEAD); wip=
if [ -n "$(git status --porcelain --untracked-files=all)" ]; then
  idx=$(mktemp); GIT_INDEX_FILE=$idx git read-tree HEAD; GIT_INDEX_FILE=$idx git add -A
  tree=$(GIT_INDEX_FILE=$idx git write-tree); rm -f $idx
  wip=$(GIT_AUTHOR_NAME=Alethiometer GIT_AUTHOR_EMAIL=alethiometer@localhost GIT_COMMITTER_NAME=Alethiometer GIT_COMMITTER_EMAIL=alethiometer@localhost git commit-tree $tree -p $head -m 'Uncommitted in the sandbox')
fi
echo head=$head; echo wip=$wip
refs=
if [ "$head" != {base} ]; then git update-ref refs/alethiometer/head $head; refs=refs/alethiometer/head; fi
if [ -n "$wip" ]; then git update-ref refs/alethiometer/wip $wip; refs="$refs refs/alethiometer/wip"; fi
if [ -n "$refs" ]; then rm -f {out}; git bundle create -q {out} $refs --not {base}; echo bundle={out}; fi
git update-ref -d refs/alethiometer/head 2>/dev/null || true; git update-ref -d refs/alethiometer/wip 2>/dev/null || true"#,
        dir = q(dir),
        base = q(base),
        out = q(out),
    )
}

#[derive(Debug, Default, PartialEq)]
pub struct Back {
    pub head: String,
    pub wip: Option<String>,
    pub bundle: Option<String>,
}

pub fn parse_back(out: &str) -> Result<Back> {
    let get = |k: &str| {
        out.lines()
            .rev()
            .find_map(|l| l.trim().strip_prefix(k).map(str::to_owned))
            .filter(|v| !v.is_empty())
    };
    Ok(Back {
        head: get("head=").ok_or_else(|| msg("couldn't read the sandbox's HEAD"))?,
        wip: get("wip="),
        bundle: get("bundle="),
    })
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Commit {
    pub sha: String,
    pub subject: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct File {
    pub path: String,
    pub what: &'static str,
}

/// Into hidden refs; returns the tip the branch would move to, None when the sandbox has nothing new.
pub fn fetch(dir: &str, bundle: &Path, back: &Back) -> Result<Option<String>> {
    if back.bundle.is_none() {
        return Ok(None);
    }
    let b = bundle.display().to_string();
    let heads = local(dir, &["bundle", "list-heads", &b])?;
    let mut args = vec!["fetch", "-q", b.as_str()];
    if heads.contains("refs/alethiometer/head") {
        args.push("+refs/alethiometer/head:refs/alethiometer/back/head");
    }
    if heads.contains("refs/alethiometer/wip") {
        args.push("+refs/alethiometer/wip:refs/alethiometer/back/wip");
    }
    local(dir, &args)?;
    Ok(Some(back.wip.clone().unwrap_or_else(|| back.head.clone())))
}

pub fn commits(dir: &str, base: &str, tip: &str) -> Result<Vec<Commit>> {
    let log = local(dir, &["log", "--format=%h%x09%s", &format!("{base}..{tip}")])?;
    Ok(log
        .lines()
        .filter_map(|l| l.split_once('\t'))
        .map(|(sha, subject)| Commit {
            sha: sha.into(),
            subject: subject.into(),
        })
        .collect())
}

pub fn files(dir: &str, base: &str, tip: &str) -> Result<Vec<File>> {
    let out = local(dir, &["diff", "--name-status", "--no-renames", base, tip])?;
    Ok(out
        .lines()
        .filter_map(|l| l.split_once('\t'))
        .map(|(s, p)| File {
            path: p.into(),
            what: match s {
                "A" => "added",
                "D" => "deleted",
                _ => "modified",
            },
        })
        .collect())
}

pub fn branch_name(sandbox: &str, folder: &str) -> String {
    format!("sandbox/{sandbox}/{folder}")
}

/// Only a branch that's new, where Bring back last left it, or behind `tip` is moved.
pub fn land(dir: &str, branch: &str, tip: &str, landed: &str) -> Result<()> {
    let r = format!("refs/heads/{branch}");
    let trees = local(dir, &["worktree", "list", "--porcelain"]).unwrap_or_default();
    if trees.lines().any(|l| l == format!("branch {r}")) {
        return Err(msg(format!(
            "{branch} is checked out; switch away from it, then bring back again"
        )));
    }
    if let Ok(now) = local(dir, &["rev-parse", "--verify", "-q", &r]) {
        let ours = now == landed || local(dir, &["merge-base", "--is-ancestor", &now, tip]).is_ok();
        if !ours {
            return Err(msg(format!(
                "{branch} has commits that aren't the sandbox's; rename or delete it, then bring back again"
            )));
        }
    }
    local(dir, &["update-ref", &r, tip]).map(|_| ())
}

pub fn identity(dir: &str) -> (String, String) {
    let get = |k: &str| local(dir, &["config", k]).unwrap_or_default();
    (get("user.name"), get("user.email"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sh(script: &str) {
        let out = Command::new("sh").arg("-c").arg(script).output().unwrap();
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    }

    fn run(dir: &str, args: &[&str]) -> String {
        local(dir, args).unwrap()
    }

    #[test]
    fn a_branch_goes_in_as_a_clone_and_comes_back_as_commits_with_the_uncommitted_on_top() {
        let t = tempfile::tempdir().unwrap();
        let here = t.path().join("app").display().to_string();
        let there = t.path().join("sandbox/app").display().to_string();
        std::fs::create_dir_all(&here).unwrap();
        run(&here, &["init", "-q", "-b", "main"]);
        run(&here, &["config", "user.name", "Me"]);
        run(&here, &["config", "user.email", "me@example.com"]);
        std::fs::write(format!("{here}/a.txt"), "one\n").unwrap();
        run(&here, &["add", "."]);
        run(&here, &["commit", "-q", "-m", "start"]);
        assert!(is_top(&here));
        assert_eq!(current_branch(&here).as_deref(), Some("main"));

        let file = t.path().join("in.bundle");
        let base = bundle(&here, "main", &file).unwrap();
        let (name, email) = identity(&here);
        sh(&clone_script(
            &file.display().to_string(),
            &there,
            "main",
            &name,
            &email,
        ));
        assert!(
            local(&there, &["remote", "get-url", "origin"]).is_err(),
            "no remote to push to"
        );
        assert!(!file.exists());

        let out = t.path().join("back.bundle").display().to_string();
        let nothing = Command::new("sh")
            .arg("-c")
            .arg(back_script(&there, &base, &out))
            .output()
            .unwrap();
        let nothing = parse_back(&String::from_utf8_lossy(&nothing.stdout)).unwrap();
        assert_eq!((nothing.wip.as_deref(), nothing.bundle.as_deref()), (None, None));

        std::fs::write(format!("{there}/a.txt"), "two\n").unwrap();
        run(&there, &["commit", "-q", "-am", "agent's work"]);
        std::fs::write(format!("{there}/a.txt"), "three\n").unwrap();
        std::fs::write(format!("{there}/new.txt"), "new\n").unwrap();
        let o = Command::new("sh")
            .arg("-c")
            .arg(back_script(&there, &base, &out))
            .output()
            .unwrap();
        assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
        let back = parse_back(&String::from_utf8_lossy(&o.stdout)).unwrap();
        assert!(back.wip.is_some());
        assert_eq!(
            run(&there, &["status", "--porcelain"]),
            " M a.txt\n?? new.txt",
            "the agent's files stay"
        );
        assert_eq!(run(&there, &["log", "-1", "--format=%s"]), "agent's work");

        let tip = fetch(&here, Path::new(&out), &back).unwrap().unwrap();
        let subjects: Vec<String> = commits(&here, &base, &tip)
            .unwrap()
            .into_iter()
            .map(|c| c.subject)
            .collect();
        assert_eq!(subjects, ["Uncommitted in the sandbox", "agent's work"]);
        assert_eq!(
            files(&here, &base, &tip).unwrap(),
            [
                File {
                    path: "a.txt".into(),
                    what: "modified"
                },
                File {
                    path: "new.txt".into(),
                    what: "added"
                }
            ]
        );
        let branch = branch_name("aleth-1", "app");
        land(&here, &branch, &tip, "").unwrap();
        assert_eq!(run(&here, &["rev-parse", &branch]), tip);
        assert_eq!(
            run(&here, &["rev-parse", "--abbrev-ref", "HEAD"]),
            "main",
            "your checkout stays"
        );

        run(&here, &["checkout", "-q", &branch]);
        assert!(
            land(&here, &branch, &back.head, &tip).is_err(),
            "not while it's checked out"
        );
        std::fs::write(format!("{here}/mine.txt"), "mine\n").unwrap();
        run(&here, &["add", "."]);
        run(&here, &["commit", "-q", "-m", "mine"]);
        run(&here, &["checkout", "-q", "main"]);
        assert!(
            land(&here, &branch, &back.head, &tip).is_err(),
            "your commits on it stay"
        );
    }
}
