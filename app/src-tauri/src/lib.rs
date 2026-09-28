pub mod ai;
pub mod api;
pub mod clock;
pub mod db;
pub mod error;
pub mod outbox;
pub mod platform;
pub mod queries;
pub mod seed;
pub mod settings;
pub mod state;
pub mod sync;

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
        let app = Arc::new(App::open(&App::default_data_dir()).expect("failed to open Alethiometer data directory"));
        crate::sync::start(app.clone());
        crate::outbox::start(app.clone());
        let scale = app.settings.ui_scale.clamp(0.5, 3.0);
        tauri::Builder::default()
            .setup(move |a| {
                use tauri::Manager;
                if let Some(w) = a.get_webview_window("main") {
                    let _ = w.set_zoom(scale);
                }
                Ok(())
            })
            .manage(app)
            .invoke_handler(tauri::generate_handler![call])
            .run(tauri::generate_context!())
            .expect("error while running Alethiometer");
    }
}

#[cfg(feature = "desktop")]
pub use desktop::run;
