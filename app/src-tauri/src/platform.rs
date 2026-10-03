//! Mac-only integrations behind one trait: secrets (Keychain), global hotkey,
//! notifications, launch at login, tray. Linux (cloud dev) gets stubs; secrets
//! go to a 0600 file.
//!
//! TODO(mac): Keychain via `security-framework`, global hotkey and tray via
//! Tauri plugins, notifications for the Reflect nudge (9d). Until then macOS
//! uses the same file-backed stub.

use std::collections::BTreeMap;
use std::path::PathBuf;

pub trait Platform: Send + Sync {
    fn secret_get(&self, key: &str) -> Option<String>;
    fn secret_set(&self, key: &str, value: &str) -> std::io::Result<()>;
    /// When a secret was last set ("2026-09-14T10:02:00"), if known.
    fn secret_set_at(&self, _key: &str) -> Option<String> {
        None
    }
    /// Persistent notification (stays until answered). Returns false if unsupported.
    fn notify_persistent(&self, _title: &str, _body: &str) -> bool {
        false
    }
    fn set_tray_dot(&self, _on: bool) {}
    /// Open a URL in the default browser (consent screens, "open in app").
    fn open_url(&self, url: &str) -> std::io::Result<()> {
        let cmd = if cfg!(target_os = "macos") { "open" } else { "xdg-open" };
        std::process::Command::new(cmd).arg(url).spawn().map(|_| ())
    }
    /// A terminal window in `cwd`, running `command` if given (attach to an agent).
    fn open_terminal(&self, cwd: &str, command: Option<&str>) -> std::io::Result<()> {
        let quote = |s: &str| format!("'{}'", s.replace('\'', r"'\''"));
        let mut line = String::new();
        if !cwd.is_empty() {
            line.push_str(&format!("cd {}", quote(cwd)));
        }
        if let Some(c) = command {
            if !line.is_empty() {
                line.push_str(" && ");
            }
            line.push_str(c);
        }
        if cfg!(target_os = "macos") {
            let script = format!("tell application \"Terminal\" to do script \"{}\"\ntell application \"Terminal\" to activate", line.replace('\\', "\\\\").replace('"', "\\\""));
            std::process::Command::new("osascript").arg("-e").arg(script).spawn().map(|_| ())
        } else {
            let line = if line.is_empty() { "exec $SHELL".to_string() } else { format!("{line}; exec $SHELL") };
            std::process::Command::new("x-terminal-emulator").arg("-e").arg("sh").arg("-c").arg(line).spawn().map(|_| ())
        }
    }
    /// Open `path` in an app by name ("Zed"), falling back to its CLI.
    fn open_in_app(&self, app: &str, path: &str) -> std::io::Result<()> {
        if cfg!(target_os = "macos") {
            std::process::Command::new("open").arg("-a").arg(app).arg(path).spawn().map(|_| ())
        } else {
            std::process::Command::new(app.to_lowercase()).arg(path).spawn().map(|_| ())
        }
    }
}

/// Secrets in `<data dir>/secrets.json`, mode 0600. When each was set goes in
/// `secrets.meta.json` (names and times only), for the Settings page.
///
/// Writes are serialized and atomic (`fsx::write_atomic`). A file that's
/// there but doesn't parse is never written over: rewriting it from an
/// empty map would silently drop every other secret.
pub struct FileSecrets {
    path: PathBuf,
    meta: PathBuf,
    write: std::sync::Mutex<()>,
}

impl FileSecrets {
    pub fn new(data_dir: &std::path::Path) -> FileSecrets {
        FileSecrets { path: data_dir.join("secrets.json"), meta: data_dir.join("secrets.meta.json"), write: Default::default() }
    }

    fn read_meta(&self) -> BTreeMap<String, String> {
        std::fs::read_to_string(&self.meta).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
    }

    /// The secrets; none when the file doesn't exist yet.
    fn read(&self) -> std::io::Result<BTreeMap<String, String>> {
        match std::fs::read_to_string(&self.path) {
            Ok(t) => serde_json::from_str(&t).map_err(|e| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("{} doesn't parse ({e}); fix it by hand or move it aside, nothing was written", self.path.display()),
                )
            }),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(BTreeMap::new()),
            Err(e) => Err(e),
        }
    }
}

impl Platform for FileSecrets {
    fn secret_get(&self, key: &str) -> Option<String> {
        match self.read() {
            Ok(all) => all.get(key).cloned(),
            Err(e) => {
                eprintln!("secrets: {e}");
                None
            }
        }
    }

    fn secret_set(&self, key: &str, value: &str) -> std::io::Result<()> {
        let _w = self.write.lock().unwrap_or_else(|e| e.into_inner());
        let mut all = self.read()?;
        all.insert(key.into(), value.into());
        crate::fsx::write_atomic(&self.path, serde_json::to_string_pretty(&all)?.as_bytes(), Some(0o600))?;
        let mut meta = self.read_meta();
        meta.insert(key.into(), crate::time::Ts::new(jiff::Timestamp::now()).to_db());
        crate::fsx::write_atomic(&self.meta, serde_json::to_string_pretty(&meta)?.as_bytes(), Some(0o600))?;
        Ok(())
    }

    fn secret_set_at(&self, key: &str) -> Option<String> {
        self.read_meta().get(key).cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_that_doesnt_parse_is_never_written_over() {
        let d = tempfile::tempdir().unwrap();
        let s = FileSecrets::new(d.path());
        s.secret_set("a", "1").unwrap();
        s.secret_set("b", "2").unwrap();
        // A hand edit gone wrong (or a crash from before writes were atomic).
        std::fs::write(d.path().join("secrets.json"), "{ \"a\": \"1\", \"b\": ").unwrap();
        assert!(s.secret_set("c", "3").is_err());
        assert_eq!(std::fs::read_to_string(d.path().join("secrets.json")).unwrap(), "{ \"a\": \"1\", \"b\": ", "left for the user to fix");
        assert_eq!(s.secret_get("a"), None);
    }

    #[test]
    fn concurrent_sets_keep_every_key() {
        let d = tempfile::tempdir().unwrap();
        let s = std::sync::Arc::new(FileSecrets::new(d.path()));
        let threads: Vec<_> = (0..8)
            .map(|i| {
                let s = s.clone();
                std::thread::spawn(move || {
                    for j in 0..10 {
                        s.secret_set(&format!("k{i}.{j}"), "v").unwrap();
                    }
                })
            })
            .collect();
        for t in threads {
            t.join().unwrap();
        }
        for i in 0..8 {
            for j in 0..10 {
                assert_eq!(s.secret_get(&format!("k{i}.{j}")).as_deref(), Some("v"), "k{i}.{j}");
            }
        }
    }

    #[cfg(unix)]
    #[test]
    fn the_file_is_private_from_the_start() {
        use std::os::unix::fs::PermissionsExt;
        let d = tempfile::tempdir().unwrap();
        FileSecrets::new(d.path()).secret_set("a", "1").unwrap();
        assert_eq!(std::fs::metadata(d.path().join("secrets.json")).unwrap().permissions().mode() & 0o777, 0o600);
    }
}
