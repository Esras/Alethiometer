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
        open::that_detached(url)
    }
    /// A terminal window in `cwd`, running `command` if given (attach to an agent).
    fn open_terminal(&self, cwd: &str, command: Option<&str>) -> std::io::Result<()> {
        #[cfg(windows)]
        {
            windows_terminal(cwd, command)
        }
        #[cfg(not(windows))]
        {
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
                let script = format!(
                    "tell application \"Terminal\" to do script \"{}\"\ntell application \"Terminal\" to activate",
                    line.replace('\\', "\\\\").replace('"', "\\\"")
                );
                std::process::Command::new("osascript")
                    .arg("-e")
                    .arg(script)
                    .spawn()
                    .map(|_| ())
            } else {
                let line = if line.is_empty() {
                    "exec $SHELL".to_string()
                } else {
                    format!("{line}; exec $SHELL")
                };
                std::process::Command::new("x-terminal-emulator")
                    .arg("-e")
                    .arg("sh")
                    .arg("-c")
                    .arg(line)
                    .spawn()
                    .map(|_| ())
            }
        }
    }

    /// Open `path` in an app by name ("Zed"), falling back to its CLI.
    fn open_in_app(&self, app: &str, path: &str) -> std::io::Result<()> {
        if cfg!(target_os = "macos") {
            std::process::Command::new("open")
                .arg("-a")
                .arg(app)
                .arg(path)
                .spawn()
                .map(|_| ())
        } else {
            std::process::Command::new(app.to_lowercase())
                .arg(path)
                .spawn()
                .map(|_| ())
        }
    }
}

/// A console window of its own running cmd in `cwd`, `command` first if
/// given (/K keeps the window open after it). Windows 11 opens it in Windows
/// Terminal when that is the default terminal.
#[cfg(windows)]
fn windows_terminal(cwd: &str, command: Option<&str>) -> std::io::Result<()> {
    use std::os::windows::process::CommandExt;
    let mut c = std::process::Command::new("cmd.exe");
    c.creation_flags(windows_sys::Win32::System::Threading::CREATE_NEW_CONSOLE);
    if !cwd.is_empty() {
        c.current_dir(cwd);
    }
    if let Some(line) = command {
        // As written: cmd.exe's quoting isn't Rust's. /S strips only the outer quotes.
        c.raw_arg(format!("/S /K \"{line}\""));
    }
    c.spawn().map(|_| ())
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
        FileSecrets {
            path: data_dir.join("secrets.json"),
            meta: data_dir.join("secrets.meta.json"),
            write: Default::default(),
        }
    }

    fn read_meta(&self) -> BTreeMap<String, String> {
        std::fs::read_to_string(&self.meta)
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default()
    }

    /// The secrets; none when the file doesn't exist yet.
    fn read(&self) -> std::io::Result<BTreeMap<String, String>> {
        match std::fs::read_to_string(&self.path) {
            Ok(t) => serde_json::from_str(&t).map_err(|e| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!(
                        "{} doesn't parse ({e}); fix it by hand or move it aside, nothing was written",
                        self.path.display()
                    ),
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
        meta.insert(
            key.into(),
            alethiometer_core::time::Ts::new(jiff::Timestamp::now()).to_db(),
        );
        crate::fsx::write_atomic(&self.meta, serde_json::to_string_pretty(&meta)?.as_bytes(), Some(0o600))?;
        Ok(())
    }

    fn secret_set_at(&self, key: &str) -> Option<String> {
        self.read_meta().get(key).cloned()
    }
}

#[cfg(test)]
mod tests;
