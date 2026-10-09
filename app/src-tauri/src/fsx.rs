//! File writes that a crash, a concurrent writer or a reader can't see half of.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use std::sync::atomic::{AtomicU64, Ordering};

/// Write `bytes` to `path` through a temp file in the same directory: created
/// fresh (with `mode` from the start on Unix, so a secret is never readable
/// by others, even briefly), fsynced, then renamed over `path`. Each call
/// gets its own temp name, so two writers never rename each other's half
/// file into place; the last rename wins whole.
pub fn write_atomic(path: &Path, bytes: &[u8], mode: Option<u32>) -> std::io::Result<()> {
    let (dir, tmp) = write_temp(path, bytes, mode, None)?;
    let result = std::fs::rename(&tmp, path);
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result?;
    sync_dir(dir);
    Ok(())
}

/// Write `bytes` to `path` only if nothing is there, whole or not at all: the
/// temp file is hard-linked into place, which fails with `AlreadyExists`
/// rather than replace a file that appeared meanwhile. `modified` sets the
/// file's mtime (a copied note keeps its own).
pub fn write_new(path: &Path, bytes: &[u8], modified: Option<SystemTime>) -> std::io::Result<()> {
    let (dir, tmp) = write_temp(path, bytes, None, modified)?;
    let result = std::fs::hard_link(&tmp, path);
    let _ = std::fs::remove_file(&tmp);
    result?;
    sync_dir(dir);
    Ok(())
}

/// A fresh, fsynced temp file next to `path`. Each call gets its own name,
/// so two writers never touch each other's half file.
fn write_temp<'a>(path: &'a Path, bytes: &[u8], mode: Option<u32>, modified: Option<SystemTime>) -> std::io::Result<(&'a Path, PathBuf)> {
    static N: AtomicU64 = AtomicU64::new(0);
    let dir = path.parent().filter(|d| !d.as_os_str().is_empty()).unwrap_or(Path::new("."));
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let tmp = dir.join(format!(".{name}.{}.{}.tmp", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    if let Some(m) = mode {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(m);
    }
    #[cfg(not(unix))]
    let _ = mode;
    let result = (|| {
        let mut f = opts.open(&tmp)?;
        f.write_all(bytes)?;
        if let Some(t) = modified {
            f.set_modified(t)?;
        }
        f.sync_all()
    })();
    if let Err(e) = result {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    Ok((dir, tmp))
}

/// Make a rename or link in `dir` durable (best effort: not every platform
/// can open a directory).
fn sync_dir(dir: &Path) {
    if let Ok(d) = std::fs::File::open(dir) {
        let _ = d.sync_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replaces_whole_and_leaves_no_temp_files() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("a.json");
        write_atomic(&p, b"one", None).unwrap();
        write_atomic(&p, b"two", None).unwrap();
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "two");
        assert_eq!(std::fs::read_dir(d.path()).unwrap().count(), 1);
    }

    #[test]
    fn write_new_never_replaces_and_keeps_the_mtime() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("n.md");
        let then = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000);
        write_new(&p, b"first", Some(then)).unwrap();
        assert_eq!(std::fs::metadata(&p).unwrap().modified().unwrap(), then);
        let e = write_new(&p, b"second", None).unwrap_err();
        assert_eq!(e.kind(), std::io::ErrorKind::AlreadyExists);
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "first");
        assert_eq!(std::fs::read_dir(d.path()).unwrap().count(), 1, "no temp file left");
    }

    #[test]
    fn concurrent_write_new_has_one_winner() {
        let d = tempfile::tempdir().unwrap();
        let p = std::sync::Arc::new(d.path().join("r.md"));
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(8));
        let threads: Vec<_> = (0..8)
            .map(|i| {
                let (p, barrier) = (p.clone(), barrier.clone());
                std::thread::spawn(move || {
                    barrier.wait();
                    write_new(&p, format!("{i}").repeat(10_000).as_bytes(), None).is_ok()
                })
            })
            .collect();
        let wins = threads.into_iter().map(|t| t.join().unwrap()).filter(|&w| w).count();
        assert_eq!(wins, 1);
        let s = std::fs::read_to_string(&*p).unwrap();
        assert_eq!(s.len(), 10_000);
        assert!(s.chars().all(|c| c == s.chars().next().unwrap()), "one writer's whole file");
    }

    #[cfg(unix)]
    #[test]
    fn mode_applies_from_creation() {
        use std::os::unix::fs::PermissionsExt;
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("s.json");
        write_atomic(&p, b"{}", Some(0o600)).unwrap();
        assert_eq!(std::fs::metadata(&p).unwrap().permissions().mode() & 0o777, 0o600);
    }

    #[test]
    fn concurrent_writers_never_leave_a_mix() {
        let d = tempfile::tempdir().unwrap();
        let p = std::sync::Arc::new(d.path().join("c.json"));
        let threads: Vec<_> = (0..8)
            .map(|i| {
                let p = p.clone();
                std::thread::spawn(move || {
                    let body = format!("{}", i).repeat(10_000);
                    for _ in 0..20 {
                        write_atomic(&p, body.as_bytes(), None).unwrap();
                    }
                })
            })
            .collect();
        for t in threads {
            t.join().unwrap();
        }
        let s = std::fs::read_to_string(&*p).unwrap();
        assert_eq!(s.len(), 10_000);
        assert!(s.chars().all(|c| c == s.chars().next().unwrap()), "one writer's whole file");
    }
}
