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
    /// Persistent notification (stays until answered). Returns false if unsupported.
    fn notify_persistent(&self, _title: &str, _body: &str) -> bool {
        false
    }
    fn set_tray_dot(&self, _on: bool) {}
}

/// Secrets in `<data dir>/secrets.json`, mode 0600.
pub struct FileSecrets {
    path: PathBuf,
}

impl FileSecrets {
    pub fn new(data_dir: &std::path::Path) -> FileSecrets {
        FileSecrets { path: data_dir.join("secrets.json") }
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
        Ok(())
    }
}
