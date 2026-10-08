//! Bringing an agent's changes back out of its sandbox into the directory it
//! was uploaded from.
//!
//! When Alethiometer makes a sandbox it records what it uploaded (a hash per
//! file, the files OpenShell picks: `git ls-files -co --exclude-standard`, or
//! every file outside git) in `<data dir>/openshell-sync/<gateway>--<sandbox>.json`.
//! Bringing changes back downloads the sandbox's directory into the data dir,
//! then for each file compares three versions: as uploaded (the base), in the
//! sandbox, and here now. Only what the agent changed and you didn't is
//! copied or deleted; a file both changed is a conflict, left as it is and
//! listed. The base then moves to what was brought back, so the next time
//! only newer changes count. `.git` is never copied, and in a git checkout
//! nothing it ignores is (build output, `node_modules` the agent installed).
//!
//! Previewing and applying are two steps (you see the list first); applying
//! checks again that nothing here changed in between.

use super::super::model::Sandbox;
use crate::error::{msg, Result};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Mutex, OnceLock};

/// Each uploaded file's hash (`sha256:<hex>`, or `link:<target>` for a symlink), by path relative to the directory.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Manifest {
    pub files: BTreeMap<String, String>,
}

/// One file to bring back, or that couldn't be.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Change {
    pub path: String,
    /// added | modified | deleted
    pub what: &'static str,
    /// Its hash here when the plan was made (None: not here), checked again before it's touched.
    #[serde(skip)]
    pub local: Option<String>,
    /// The sandbox's version (None: deleted there).
    #[serde(skip)]
    pub remote: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct Plan {
    pub changes: Vec<Change>,
    /// Changed here too since the upload (or since the last time): left alone.
    pub conflicts: Vec<Change>,
    /// Changed in the sandbox to what's already here.
    pub same: usize,
    /// Paths whose base moves to the sandbox's version once applied (changes and `same`).
    #[serde(skip)]
    pub settle: BTreeMap<String, Option<String>>,
}

pub fn manifest_path(data_dir: &Path, sb: &Sandbox) -> PathBuf {
    let safe = |s: &str| s.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' { c } else { '_' }).collect::<String>();
    data_dir.join("openshell-sync").join(format!("{}--{}.json", safe(if sb.gateway.is_empty() { "active" } else { &sb.gateway }), safe(&sb.name)))
}

pub fn load(path: &Path) -> Manifest {
    std::fs::read(path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

pub fn save(path: &Path, m: &Manifest) -> Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| msg(e.to_string()))?;
    }
    crate::fsx::write_atomic(path, serde_json::to_vec_pretty(m)?.as_slice(), None).map_err(|e| msg(e.to_string()))
}

/// The files OpenShell uploads from `dir`: what git lists (tracked, and untracked but not ignored) inside a work tree, else every file.
pub fn local_files(dir: &Path) -> Result<Vec<String>> {
    if in_git(dir) {
        let out = git(dir).args(["ls-files", "-co", "--exclude-standard", "-z"]).output().map_err(|e| msg(e.to_string()))?;
        if !out.status.success() {
            return Err(msg(format!("git ls-files failed in {}", dir.display())));
        }
        return Ok(out.stdout.split(|b| *b == 0).filter(|p| !p.is_empty()).map(|p| String::from_utf8_lossy(p).into_owned()).filter(|p| !in_dot_git(p)).collect());
    }
    walk(dir)
}

fn in_git(dir: &Path) -> bool {
    git(dir).args(["rev-parse", "--is-inside-work-tree"]).output().is_ok_and(|o| o.status.success() && o.stdout.starts_with(b"true"))
}

/// git in `dir`, with an inherited GIT_DIR or GIT_WORK_TREE (a hook, a devshell) cleared, as OpenShell does.
fn git(dir: &Path) -> Command {
    let mut c = Command::new("git");
    c.current_dir(dir).stdin(Stdio::null()).stderr(Stdio::null());
    for v in ["GIT_DIR", "GIT_WORK_TREE", "GIT_INDEX_FILE", "GIT_PREFIX", "GIT_COMMON_DIR"] {
        c.env_remove(v);
    }
    c
}

fn in_dot_git(p: &str) -> bool {
    p.split('/').any(|c| c == ".git")
}

/// Every file (and symlink) under `dir`, relative, `.git` left out.
fn walk(dir: &Path) -> Result<Vec<String>> {
    let mut out = vec![];
    let mut stack = vec![PathBuf::new()];
    while let Some(rel) = stack.pop() {
        let entries = std::fs::read_dir(dir.join(&rel)).map_err(|e| msg(format!("{}: {e}", dir.join(&rel).display())))?;
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if name == ".git" {
                continue;
            }
            let path = rel.join(&name);
            let ft = e.file_type().map_err(|e| msg(e.to_string()))?;
            if ft.is_dir() {
                stack.push(path);
            } else {
                out.push(path.to_string_lossy().replace('\\', "/"));
            }
        }
    }
    out.sort();
    Ok(out)
}

/// A file's hash, a symlink's target, or None when it isn't there.
pub fn hash(path: &Path) -> Option<String> {
    let meta = std::fs::symlink_metadata(path).ok()?;
    if meta.file_type().is_symlink() {
        return Some(format!("link:{}", std::fs::read_link(path).ok()?.display()));
    }
    if !meta.is_file() {
        return None;
    }
    let bytes = std::fs::read(path).ok()?;
    let d = ring::digest::digest(&ring::digest::SHA256, &bytes);
    Some(format!("sha256:{}", d.as_ref().iter().map(|b| format!("{b:02x}")).collect::<String>()))
}

/// What was uploaded from `dir`, as it is now (taken right after the upload).
pub fn snapshot(dir: &Path) -> Result<Manifest> {
    let files = local_files(dir)?.into_iter().filter_map(|p| Some((p.clone(), hash(&dir.join(&p))?))).collect();
    Ok(Manifest { files })
}

/// Of `paths` (relative to `dir`), those git would ignore there. Tracked files never are.
fn ignored(dir: &Path, paths: &[String]) -> BTreeSet<String> {
    if paths.is_empty() || !in_git(dir) {
        return BTreeSet::new();
    }
    let Ok(mut child) = git(dir).args(["check-ignore", "--stdin", "-z"]).stdin(Stdio::piped()).stdout(Stdio::piped()).spawn() else { return BTreeSet::new() };
    let input: Vec<u8> = paths.iter().flat_map(|p| p.bytes().chain([0])).collect();
    let mut stdin = child.stdin.take();
    let writer = std::thread::spawn(move || {
        use std::io::Write;
        if let Some(s) = stdin.as_mut() {
            let _ = s.write_all(&input);
        }
    });
    let out = child.wait_with_output();
    let _ = writer.join();
    // Exit 1 means nothing is ignored.
    out.map(|o| o.stdout.split(|b| *b == 0).filter(|p| !p.is_empty()).map(|p| String::from_utf8_lossy(p).into_owned()).collect()).unwrap_or_default()
}

/// Compare the base, the downloaded sandbox directory (`remote`) and the directory here (`local`).
pub fn plan(base: &Manifest, local: &Path, remote: &Path) -> Result<Plan> {
    let mut remote_files = walk(remote)?;
    let skip = ignored(local, &remote_files);
    remote_files.retain(|p| !skip.contains(p));
    let remote_set: BTreeSet<&String> = remote_files.iter().collect();
    let paths: BTreeSet<String> = base.files.keys().cloned().chain(remote_files.iter().cloned()).collect();
    let mut p = Plan::default();
    for path in paths {
        let b = base.files.get(&path).cloned();
        let s = if remote_set.contains(&path) { hash(&remote.join(&path)) } else { None };
        if s == b {
            continue;
        }
        let l = hash(&local.join(&path));
        let what = match (&b, &s) {
            (_, None) => "deleted",
            (None, Some(_)) => "added",
            _ => "modified",
        };
        let c = Change { path: path.clone(), what, local: l.clone(), remote: s.clone() };
        if l == s {
            p.same += 1;
            p.settle.insert(path, s);
        } else if l == b {
            p.settle.insert(path, s);
            p.changes.push(c);
        } else {
            p.conflicts.push(c);
        }
    }
    Ok(p)
}

/// Copy and delete what the plan says, each file only if it's still as it
/// was when planned (else it's a conflict now); with `force` (the sandbox
/// wins) the conflicts too. Returns the paths applied, the conflicts left
/// (the plan's and new ones), and the base to keep.
pub fn apply(plan: &Plan, base: &Manifest, local: &Path, remote: &Path, force: bool) -> (Vec<Change>, Vec<Change>, Manifest) {
    let mut done = vec![];
    let mut conflicts = if force { vec![] } else { plan.conflicts.clone() };
    let mut next = base.clone();
    let todo: Vec<&Change> = plan.changes.iter().chain(plan.conflicts.iter().filter(|_| force)).collect();
    for c in todo {
        let target = local.join(&c.path);
        if hash(&target) != c.local {
            conflicts.push(c.clone());
            continue;
        }
        let ok = match &c.remote {
            None => std::fs::remove_file(&target).map(|_| remove_empty_parents(local, &target)),
            Some(_) => put(&remote.join(&c.path), &target),
        };
        match ok {
            Ok(()) => {
                match &c.remote {
                    Some(h) => next.files.insert(c.path.clone(), h.clone()),
                    None => next.files.remove(&c.path),
                };
                done.push(c.clone());
            }
            Err(e) => {
                eprintln!("agents: couldn't bring back {}: {e}", c.path);
                conflicts.push(c.clone());
            }
        }
    }
    for (path, h) in plan.settle.iter().filter(|(p, _)| !plan.changes.iter().any(|c| &c.path == *p)) {
        match h {
            Some(h) => next.files.insert(path.clone(), h.clone()),
            None => next.files.remove(path),
        };
    }
    (done, conflicts, next)
}

/// Write the sandbox's version over this one: a file through a temporary next to it, renamed into place; a symlink as a symlink.
fn put(from: &Path, to: &Path) -> std::io::Result<()> {
    if let Some(dir) = to.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let meta = std::fs::symlink_metadata(from)?;
    if meta.file_type().is_symlink() {
        let target = std::fs::read_link(from)?;
        let _ = std::fs::remove_file(to);
        #[cfg(unix)]
        return std::os::unix::fs::symlink(target, to);
        #[cfg(not(unix))]
        return Err(std::io::Error::other(format!("can't make the symlink to {}", target.display())));
    }
    let tmp = to.with_file_name(format!(".{}.alethiometer-tmp", to.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()));
    std::fs::copy(from, &tmp)?;
    std::fs::rename(&tmp, to)
}

/// Directories a deletion left empty, up to (not including) `root`.
fn remove_empty_parents(root: &Path, file: &Path) {
    let mut dir = file.parent();
    while let Some(d) = dir.filter(|d| d.starts_with(root) && *d != root) {
        if std::fs::remove_dir(d).is_err() {
            break;
        }
        dir = d.parent();
    }
}

/// A preview waiting for Copy back: the download and its plan, per session.
pub struct Preview {
    pub dir: PathBuf,
    pub plan: Plan,
    /// The session's turns when it was made, recorded as synced once applied.
    pub turns: u64,
}

pub fn previews() -> &'static Mutex<HashMap<String, Preview>> {
    static P: OnceLock<Mutex<HashMap<String, Preview>>> = OnceLock::new();
    P.get_or_init(Default::default)
}

/// Drop a session's preview and its download.
pub fn discard(session: &str) {
    if let Some(p) = previews().lock().unwrap_or_else(|e| e.into_inner()).remove(session) {
        let _ = std::fs::remove_dir_all(&p.dir);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(dir: &Path, rel: &str, text: &str) {
        let p = dir.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }

    fn paths(cs: &[Change]) -> Vec<(&str, &str)> {
        cs.iter().map(|c| (c.path.as_str(), c.what)).collect()
    }

    #[test]
    fn only_what_the_agent_changed_and_you_didnt_comes_back() {
        let local = tempfile::tempdir().unwrap();
        let remote = tempfile::tempdir().unwrap();
        for (p, t) in [("a.txt", "a"), ("b.txt", "b"), ("c.txt", "c"), ("d.txt", "d"), ("src/e.rs", "e"), ("f.txt", "f")] {
            write(local.path(), p, t);
            write(remote.path(), p, t);
        }
        let base = snapshot(local.path()).unwrap();
        write(remote.path(), "a.txt", "a2"); // the agent changed it
        write(remote.path(), "b.txt", "b2"); // both changed it
        write(local.path(), "b.txt", "b-mine");
        write(local.path(), "c.txt", "c-mine"); // only you changed it
        std::fs::remove_file(remote.path().join("src/e.rs")).unwrap(); // the agent deleted it
        write(remote.path(), "new/g.txt", "g"); // the agent added it
        write(remote.path(), "f.txt", "f2"); // both made the same change
        write(local.path(), "f.txt", "f2");
        write(remote.path(), ".git/HEAD", "ref: refs/heads/agent"); // the sandbox's own repo

        let p = plan(&base, local.path(), remote.path()).unwrap();
        assert_eq!(paths(&p.changes), [("a.txt", "modified"), ("new/g.txt", "added"), ("src/e.rs", "deleted")]);
        assert_eq!(paths(&p.conflicts), [("b.txt", "modified")]);
        assert_eq!(p.same, 1);

        let (done, conflicts, next) = apply(&p, &base, local.path(), remote.path(), false);
        assert_eq!(done.len(), 3);
        assert_eq!(paths(&conflicts), [("b.txt", "modified")]);
        assert_eq!(std::fs::read_to_string(local.path().join("a.txt")).unwrap(), "a2");
        assert_eq!(std::fs::read_to_string(local.path().join("b.txt")).unwrap(), "b-mine", "a conflict is left as it is");
        assert_eq!(std::fs::read_to_string(local.path().join("c.txt")).unwrap(), "c-mine");
        assert_eq!(std::fs::read_to_string(local.path().join("new/g.txt")).unwrap(), "g");
        assert!(!local.path().join("src").exists(), "the deleted file, and the directory it left empty");
        assert!(!local.path().join(".git").exists(), ".git never comes back");

        // The base moved: nothing new to bring back, but the conflict stays one.
        let again = plan(&next, local.path(), remote.path()).unwrap();
        assert!(again.changes.is_empty(), "{:?}", again.changes);
        assert_eq!(paths(&again.conflicts), [("b.txt", "modified")]);
    }

    #[test]
    fn a_file_changed_after_the_preview_isnt_overwritten() {
        let local = tempfile::tempdir().unwrap();
        let remote = tempfile::tempdir().unwrap();
        write(local.path(), "a.txt", "a");
        write(remote.path(), "a.txt", "a");
        let base = snapshot(local.path()).unwrap();
        write(remote.path(), "a.txt", "agent");
        let p = plan(&base, local.path(), remote.path()).unwrap();
        write(local.path(), "a.txt", "typed in the meantime");
        let (done, conflicts, next) = apply(&p, &base, local.path(), remote.path(), false);
        assert!(done.is_empty());
        assert_eq!(paths(&conflicts), [("a.txt", "modified")]);
        assert_eq!(std::fs::read_to_string(local.path().join("a.txt")).unwrap(), "typed in the meantime");
        assert_eq!(next, base, "the base stays where it was");
    }

    #[test]
    fn the_sandbox_wins_when_asked() {
        let local = tempfile::tempdir().unwrap();
        let remote = tempfile::tempdir().unwrap();
        write(local.path(), "a.txt", "a");
        write(remote.path(), "a.txt", "a");
        let base = snapshot(local.path()).unwrap();
        write(remote.path(), "a.txt", "agent");
        write(local.path(), "a.txt", "mine");
        let p = plan(&base, local.path(), remote.path()).unwrap();
        assert_eq!(paths(&p.conflicts), [("a.txt", "modified")]);
        let (done, conflicts, next) = apply(&p, &base, local.path(), remote.path(), true);
        assert_eq!((done.len(), conflicts.len()), (1, 0));
        assert_eq!(std::fs::read_to_string(local.path().join("a.txt")).unwrap(), "agent");
        assert!(plan(&next, local.path(), remote.path()).unwrap().conflicts.is_empty());
    }

    #[test]
    fn in_git_ignored_files_stay_in_the_sandbox() {
        let local = tempfile::tempdir().unwrap();
        let remote = tempfile::tempdir().unwrap();
        let ok = Command::new("git").args(["init", "-q"]).current_dir(local.path()).status().is_ok_and(|s| s.success());
        if !ok {
            eprintln!("skipped: no git");
            return;
        }
        write(local.path(), ".gitignore", "node_modules/\n*.log\n");
        write(local.path(), "src/main.js", "1");
        write(local.path(), "debug.log", "local log");
        let base = snapshot(local.path()).unwrap();
        assert_eq!(base.files.keys().map(String::as_str).collect::<Vec<_>>(), [".gitignore", "src/main.js"], "what OpenShell uploads");
        write(remote.path(), ".gitignore", "node_modules/\n*.log\n");
        write(remote.path(), "src/main.js", "2");
        write(remote.path(), "node_modules/x/index.js", "dep");
        write(remote.path(), "run.log", "noise");
        let p = plan(&base, local.path(), remote.path()).unwrap();
        assert_eq!(paths(&p.changes), [("src/main.js", "modified")]);
    }

    #[test]
    fn manifests_are_named_per_gateway_and_sandbox() {
        let sb = Sandbox { gateway: "my gw/1".into(), name: "aleth-1".into(), ..Default::default() };
        assert_eq!(manifest_path(Path::new("/d"), &sb), Path::new("/d/openshell-sync/my_gw_1--aleth-1.json"));
        let active = Sandbox { name: "aleth-1".into(), ..Default::default() };
        assert_eq!(manifest_path(Path::new("/d"), &active), Path::new("/d/openshell-sync/active--aleth-1.json"));
    }
}
