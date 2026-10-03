pub mod agents;
pub mod ai;
pub mod api;
pub mod clock;
/// The schema and migrations (`store::schema`), under its old name.
pub use store::schema as db;
pub mod domain;
pub mod embed;
pub mod error;
pub mod fsx;
pub mod glean;
pub mod outbox;
pub mod platform;
pub mod queries;
pub mod seed;
pub mod settings;
pub mod state;
pub mod store;
pub mod sync;
pub mod time;

#[cfg(feature = "desktop")]
mod desktop {
    use crate::state::App;
    use serde_json::Value;
    use std::sync::Arc;

    #[tauri::command]
    async fn call(app: tauri::State<'_, Arc<App>>, cmd: String, args: Value) -> Result<Value, crate::error::Error> {
        let app = app.inner().clone();
        tauri::async_runtime::spawn_blocking(move || crate::api::call(&app, &cmd, &args))
            .await
            .map_err(|e| crate::error::msg(format!("internal: {e}")))?
    }

    pub fn run() {
        let app = App::shared(App::open(&App::default_data_dir()).expect("failed to open Alethiometer data directory"));
        crate::sync::start(app.clone());
        crate::outbox::start(app.clone());
        crate::settings::live::watch(app.clone());
        crate::embed::start(app.clone());
        crate::agents::start(app.clone());
        let scale = app.settings().ui_scale.clamp(0.5, 3.0);
        let core = app.clone();
        let on_exit = app.clone();
        tauri::Builder::default()
            .setup(move |a| {
                use tauri::Manager;
                if let Some(w) = a.get_webview_window("main") {
                    let _ = w.set_zoom(scale);
                }
                // Settings apply live: zoom in place, restart when a setting can't.
                let zoom = a.handle().clone();
                let restart = a.handle().clone();
                let _ = core.hooks.set(crate::state::Hooks {
                    set_zoom: Arc::new(move |z| {
                        if let Some(w) = zoom.get_webview_window("main") {
                            let _ = w.set_zoom(z);
                        }
                    }),
                    restart: Arc::new(move || restart.restart()),
                });
                Ok(())
            })
            .manage(app)
            .invoke_handler(tauri::generate_handler![call])
            .build(tauri::generate_context!())
            .expect("error while running Alethiometer")
            // Local agents live in the agent host and keep running after this.
            .run(move |_, e| {
                if let tauri::RunEvent::Exit = e {
                    crate::agents::shutdown(&on_exit);
                }
            });
    }
}

#[cfg(feature = "desktop")]
pub use desktop::run;
