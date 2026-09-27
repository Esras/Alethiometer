//! Process-wide state shared by the Tauri app and the dev server.

use crate::clock::Clock;
use crate::platform::{FileSecrets, Platform};
use crate::settings::Settings;
use rusqlite::Connection;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

pub struct App {
    db: Mutex<Connection>,
    pub settings: Settings,
    pub clock: Clock,
    pub data_dir: PathBuf,
    pub vault: PathBuf,
    pub platform: Box<dyn Platform>,
}

impl App {
    /// `HELM_DATA_DIR` overrides the platform data directory.
    pub fn default_data_dir() -> PathBuf {
        std::env::var_os("HELM_DATA_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| dirs::data_dir().unwrap_or_else(|| ".".into()).join("dev.helm.assistant"))
    }

    pub fn open(data_dir: &Path) -> anyhow::Result<App> {
        std::fs::create_dir_all(data_dir)?;
        let settings = Settings::load_or_init(&data_dir.join("settings.json"))?;
        let conn = crate::db::open(&data_dir.join("helm.sqlite"))?;
        Self::from_parts(conn, settings, data_dir)
    }

    pub fn from_parts(conn: Connection, settings: Settings, data_dir: &Path) -> anyhow::Result<App> {
        let vault = if settings.obsidian_vault.is_empty() {
            data_dir.join("vault")
        } else {
            PathBuf::from(&settings.obsidian_vault)
        };
        // HELM_FAKE_NOW (e.g. "10:52") overrides settings.fake_now for dev and screenshots.
        let fake = std::env::var("HELM_FAKE_NOW").unwrap_or_else(|_| settings.fake_now.clone());
        let clock = Clock::new(&fake);
        let app = App {
            db: Mutex::new(conn),
            clock,
            data_dir: data_dir.to_path_buf(),
            vault,
            platform: Box::new(FileSecrets::new(data_dir)),
            settings,
        };
        crate::sync::register_sources(&app.db(), &app.settings.sources)?;
        if app.settings.demo_data {
            crate::seed::seed_if_empty(&app)?;
        }
        Ok(app)
    }

    pub fn db(&self) -> MutexGuard<'_, Connection> {
        self.db.lock().unwrap_or_else(|e| e.into_inner())
    }
}
