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
pub struct FileSecrets {
    path: PathBuf,
    meta: PathBuf,
}

impl FileSecrets {
    pub fn new(data_dir: &std::path::Path) -> FileSecrets {
        FileSecrets { path: data_dir.join("secrets.json"), meta: data_dir.join("secrets.meta.json") }
    }

    fn read_meta(&self) -> BTreeMap<String, String> {
        std::fs::read_to_string(&self.meta).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
    }

    fn read(&self) -> BTreeMap<String, String> {
        std::fs::read_to_string(&self.path)
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default()
    }
}

impl Platform for FileSecrets {
    fn secret_get(&self, key: &str) -> Option<String> {
        self.read().get(key).cloned()
    }

    fn secret_set(&self, key: &str, value: &str) -> std::io::Result<()> {
        let mut all = self.read();
        all.insert(key.into(), value.into());
        std::fs::write(&self.path, serde_json::to_string_pretty(&all)?)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&self.path, std::fs::Permissions::from_mode(0o600))?;
        }
        let mut meta = self.read_meta();
        meta.insert(key.into(), chrono::Local::now().format("%Y-%m-%dT%H:%M:%S").to_string());
        std::fs::write(&self.meta, serde_json::to_string_pretty(&meta)?)?;
        Ok(())
    }

    fn secret_set_at(&self, key: &str) -> Option<String> {
        self.read_meta().get(key).cloned()
    }
}
