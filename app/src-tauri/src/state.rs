//! Process-wide state shared by the Tauri app and the dev server.

use crate::clock::Clock;
use crate::platform::{FileSecrets, Platform};
use crate::settings::Settings;
use rusqlite::Connection;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, RwLock};

/// What the shell around the core can do when settings change: the desktop
/// app sets these; the dev server and tests leave them unset.
pub struct Hooks {
    /// Apply a new `ui_scale` to the window.
    pub set_zoom: Arc<dyn Fn(f64) + Send + Sync>,
    /// Restart the whole app (a setting that can't apply live).
    pub restart: Arc<dyn Fn() + Send + Sync>,
}

pub struct App {
    db: crate::store::Db,
    /// Re-read from `settings.json` on every change (`crate::settings::live`).
    settings: RwLock<Arc<Settings>>,
    pub hooks: OnceLock<Hooks>,
    /// Local embeddings (`embed`): the model once loaded, and its status.
    pub embed: crate::embed::Engine,
    /// Agents tab: sessions, transcripts, local agent processes, Scion Hubs.
    pub agents: crate::agents::Agents,
    settings_state: Mutex<crate::settings::live::State>,
    /// Set by `shared`, so code holding `&App` can start background work.
    this: OnceLock<std::sync::Weak<App>>,
    pub clock: Clock,
    pub data_dir: PathBuf,
    pub vault: PathBuf,
    /// Shared so background threads (e.g. the OAuth callback) can store secrets.
    pub platform: std::sync::Arc<dyn Platform>,
}

impl App {
    /// `ALETHIOMETER_DATA_DIR` overrides the platform data directory.
    pub fn default_data_dir() -> PathBuf {
        std::env::var_os("ALETHIOMETER_DATA_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| dirs::data_dir().unwrap_or_else(|| ".".into()).join("dev.alethiometer.app"))
    }

    pub fn open(data_dir: &Path) -> anyhow::Result<App> {
        std::fs::create_dir_all(data_dir)?;
        let (settings, problem) = crate::settings::live::load_at_startup(&data_dir.join("settings.json"))?;
        let conn = crate::db::open(&data_dir.join("alethiometer.sqlite"))?;
        Self::open_with(conn, settings, data_dir, problem)
    }

    pub fn from_parts(conn: Connection, settings: Settings, data_dir: &Path) -> anyhow::Result<App> {
        let problem = settings.validate().err().map(|e| format!("settings.json: {e}"));
        Self::open_with(conn, settings, data_dir, problem)
    }

    /// `problem`: settings.json on disk isn't `settings` (it doesn't parse or
    /// validate). Nothing is written back to it then.
    fn open_with(conn: Connection, mut settings: Settings, data_dir: &Path, problem: Option<String>) -> anyhow::Result<App> {
        let vault = if settings.obsidian_vault.is_empty() {
            data_dir.join("vault")
        } else {
            PathBuf::from(&settings.obsidian_vault)
        };
        // Sources that take their config from top-level settings (vault path, Life links).
        crate::sync::connectors::obsidian::resolve_config(&mut settings.sources, &vault);
        crate::sync::connectors::protoncal::resolve_config(&mut settings);
        // ALETHIOMETER_FAKE_NOW (e.g. "10:52") overrides settings.fake_now for dev and screenshots.
        let fake = std::env::var("ALETHIOMETER_FAKE_NOW").unwrap_or_else(|_| settings.fake_now.clone());
        let clock = Clock::new(&fake);
        let app = App {
            db: crate::store::Db::new(conn)?,
            clock,
            data_dir: data_dir.to_path_buf(),
            vault,
            platform: std::sync::Arc::new(FileSecrets::new(data_dir)),
            settings: RwLock::new(Arc::new(settings)),
            hooks: OnceLock::new(),
            embed: Default::default(),
            agents: Default::default(),
            settings_state: Mutex::new(Default::default()),
            this: OnceLock::new(),
        };
        let mut problem = problem;
        let from = app.settings().categories.clone();
        match app.store().write(|uow| crate::queries::categories::load(uow, &from)) {
            // Migrated from the old form: written back, unless the file is the broken one.
            Ok(true) if problem.is_none() => {
                if let Err(e) = crate::queries::categories::save(&app, &data_dir.join("settings.json")) {
                    eprintln!("settings.json: couldn't write the migrated categories: {e}");
                }
            }
            Ok(_) => {}
            Err(e) => problem = Some(problem.map_or(format!("categories: {e}"), |p| format!("{p} · categories: {e}"))),
        }
        crate::sync::register_sources(&app.db(), &app.settings().sources)?;
        if app.settings().demo_data {
            crate::seed::seed_if_empty(&app)?;
        }
        crate::settings::live::startup(&app, problem);
        // People notes may have changed while the app was closed.
        crate::queries::people::rescan(&app)?;
        Ok(app)
    }

    /// Wrap for sharing across threads; `arc()` works from then on.
    pub fn shared(app: App) -> Arc<App> {
        let a = Arc::new(app);
        let _ = a.this.set(Arc::downgrade(&a));
        a
    }

    pub fn arc(&self) -> Option<Arc<App>> {
        self.this.get().and_then(std::sync::Weak::upgrade)
    }

    /// The current settings. Cheap; hold the `Arc` for a consistent view.
    pub fn settings(&self) -> Arc<Settings> {
        self.settings.read().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// Swap in new settings (after `settings.json` changed).
    pub fn set_settings(&self, s: Settings) {
        *self.settings.write().unwrap_or_else(|e| e.into_inner()) = Arc::new(s);
    }

    /// Change settings in memory only (tests).
    pub fn update_settings(&self, f: impl FnOnce(&mut Settings)) {
        let mut s = (*self.settings()).clone();
        f(&mut s);
        self.set_settings(s);
    }

    /// What the Settings header shows (`settings::live`).
    pub fn settings_state(&self) -> MutexGuard<'_, crate::settings::live::State> {
        self.settings_state.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn settings_path(&self) -> PathBuf {
        self.data_dir.join("settings.json")
    }

    /// The database, for the edges only: `api.rs` (per command), a sync
    /// batch, an outbox send, an agent flush. Services get a unit of work.
    pub fn store(&self) -> &crate::store::Db {
        &self.db
    }

    /// The writer connection without a transaction: the old access, for
    /// code not moved to repositories yet (`store::Db::legacy`).
    pub fn db(&self) -> crate::store::LegacyGuard<'_> {
        self.db.legacy()
    }
}
