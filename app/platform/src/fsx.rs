//! File writes that a crash, a concurrent writer or a reader can't see half of.

use std::io::Write;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

/// Write `bytes` to `path` through a temp file in the same directory: created
/// fresh (with `mode` from the start on Unix, so a secret is never readable
/// by others, even briefly), fsynced, then renamed over `path`. Each call
/// gets its own temp name, so two writers never rename each other's half
/// file into place; the last rename wins whole.
pub fn write_atomic(path: &Path, bytes: &[u8], mode: Option<u32>) -> std::io::Result<()> {
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
        f.sync_all()?;
        drop(f);
        std::fs::rename(&tmp, path)?;
        // The rename itself, durable (best effort: not every platform can open a directory).
        if let Ok(d) = std::fs::File::open(dir) {
            let _ = d.sync_all();
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
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

#[cfg(test)]
mod tests;
