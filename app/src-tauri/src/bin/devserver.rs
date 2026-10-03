//! Browser-mode bridge for UI development without WebKitGTK:
//! `POST http://127.0.0.1:1421/call/<cmd>` with a JSON body of args.
//! Uses `ALETHIOMETER_DATA_DIR` (default `./.alethiometer-dev`).

use alethiometer_lib::state::App;
use tiny_http::{Header, Response, Server};

fn main() -> anyhow::Result<()> {
    let dir: std::path::PathBuf = std::env::var_os("ALETHIOMETER_DATA_DIR").map(Into::into).unwrap_or_else(|| ".alethiometer-dev".into());
    let app = App::shared(App::open(&dir)?);
    alethiometer_lib::sync::start(app.clone());
    alethiometer_lib::settings::live::watch(app.clone());
    alethiometer_lib::embed::start(app.clone());
    alethiometer_lib::outbox::start(app.clone());
    alethiometer_lib::agents::start(app.clone());
    let addr = std::env::var("ALETHIOMETER_DEVSERVER_ADDR").unwrap_or_else(|_| "127.0.0.1:1421".into());
    let server = Server::http(&addr).map_err(|e| anyhow::anyhow!("{e}"))?;
    eprintln!("alethiometer devserver on http://{addr} (data: {})", app.data_dir.display());
    let cors = Header::from_bytes("Access-Control-Allow-Origin", "*").unwrap();
    let cors_h = Header::from_bytes("Access-Control-Allow-Headers", "content-type").unwrap();
    let json_h = Header::from_bytes("Content-Type", "application/json").unwrap();
    for mut req in server.incoming_requests() {
        if req.method() == &tiny_http::Method::Options {
            let _ = req.respond(Response::empty(204).with_header(cors.clone()).with_header(cors_h.clone()));
            continue;
        }
        let cmd = req.url().trim_start_matches("/call/").to_string();
        let mut body = String::new();
        let _ = req.as_reader().read_to_string(&mut body);
        let args: serde_json::Value = serde_json::from_str(&body).unwrap_or(serde_json::Value::Null);
        let (status, out) = match alethiometer_lib::api::call(&app, &cmd, &args) {
            Ok(v) => (200, v),
            Err(e) => (400, serde_json::json!({ "error": e.to_string() })),
        };
        let resp = Response::from_string(out.to_string())
            .with_status_code(status)
            .with_header(cors.clone())
            .with_header(json_h.clone());
        let _ = req.respond(resp);
    }
    Ok(())
}
