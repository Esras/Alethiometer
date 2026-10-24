//! File writes that a crash, a concurrent writer or a reader can't see half of.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::SystemTime;

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
fn write_temp<'a>(
    path: &'a Path,
    bytes: &[u8],
    mode: Option<u32>,
    modified: Option<SystemTime>,
) -> std::io::Result<(&'a Path, PathBuf)> {
    static N: AtomicU64 = AtomicU64::new(0);
    let dir = path
        .parent()
        .filter(|d| !d.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let tmp = dir.join(format!(
        ".{name}.{}.{}.tmp",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ));
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

/// Write `after` over `abs` via temp file + rename, but only if the file still
/// holds `before` (None = must not exist yet). Returns the new mtime.
pub fn write_checked(abs: &Path, before: Option<&str>, after: &str) -> anyhow::Result<std::time::SystemTime> {
    use std::io::Write;
    let name = abs
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    // Hidden name: the scanner and the watcher filter skip it.
    let tmp = abs.with_file_name(format!(".{name}.alethiometer-tmp"));
    {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(after.as_bytes())?;
        f.sync_all()?;
    }
    let unchanged = match before {
        Some(b) => std::fs::read(abs).ok().as_deref() == Some(b.as_bytes()),
        None => !abs.exists(),
    };
    if !unchanged {
        let _ = std::fs::remove_file(&tmp);
        anyhow::bail!("the note changed while writing; retry");
    }
    if let Ok(meta) = std::fs::metadata(abs) {
        let _ = std::fs::set_permissions(&tmp, meta.permissions());
    }
    if let Err(e) = std::fs::rename(&tmp, abs) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e.into());
    }
    Ok(std::fs::metadata(abs)?.modified()?)
}

/// Move a vault note into the vault's `.trash/` (where Obsidian's "Move to
/// Obsidian trash" puts deleted files), but only if it still holds `before`.
/// A name already in the trash gets " 1", " 2"… before `.md`; the note is
/// hard-linked in under a free name first, so nothing there is replaced.
/// Returns where it went.
pub fn move_to_trash(vault: &Path, abs: &Path, before: &str) -> anyhow::Result<PathBuf> {
    if std::fs::read(abs)?.as_slice() != before.as_bytes() {
        anyhow::bail!("the note changed; it wasn't moved to the trash");
    }
    let trash = vault.join(".trash");
    std::fs::create_dir_all(&trash)?;
    let stem = abs
        .file_stem()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let ext = abs
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default();
    for n in 0.. {
        let to = trash.join(if n == 0 {
            format!("{stem}{ext}")
        } else {
            format!("{stem} {n}{ext}")
        });
        match std::fs::hard_link(abs, &to) {
            Ok(()) => {
                std::fs::remove_file(abs)?;
                sync_dir(&trash);
                if let Some(dir) = abs.parent() {
                    sync_dir(dir);
                }
                return Ok(to);
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e.into()),
        }
    }
    unreachable!()
}

#[cfg(test)]
mod tests;
