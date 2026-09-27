pub mod ai;
pub mod api;
pub mod clock;
pub mod db;
pub mod error;
pub mod platform;
pub mod queries;
pub mod seed;
pub mod settings;
pub mod state;

#[cfg(feature = "desktop")]
mod desktop {
    use crate::state::App;
    use serde_json::Value;

    #[tauri::command]
    fn call(app: tauri::State<'_, App>, cmd: String, args: Value) -> Result<Value, crate::error::Error> {
        crate::api::call(&app, &cmd, &args)
    }

    pub fn run() {
        let app = App::open(&App::default_data_dir()).expect("failed to open Helm data directory");
        tauri::Builder::default()
            .manage(app)
            .invoke_handler(tauri::generate_handler![call])
            .run(tauri::generate_context!())
            .expect("error while running Helm");
    }
}

#[cfg(feature = "desktop")]
pub use desktop::run;
