//! Reflect view. TODO: implement.

use crate::api::unknown;
use crate::error::Result;
use crate::seed::Ctx;
use crate::state::App;
use serde_json::Value;

pub fn call(_app: &App, name: &str, _args: &Value) -> Result<Value> {
    Err(unknown(name))
}

pub fn seed(_ctx: &Ctx, _vault: &std::path::Path) -> anyhow::Result<()> {
    Ok(())
}
