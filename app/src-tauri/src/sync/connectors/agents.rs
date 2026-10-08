//! Writes for the built-in `agents` source (Incoming's agent signals): today
//! deleting an OpenShell sandbox when its session is archived. Never polled.

use crate::sync::{Batch, Connector, Ctx};
use serde_json::{json, Value};

pub struct Agents;

impl Connector for Agents {
    fn poll(&self, _ctx: &Ctx) -> anyhow::Result<Batch> {
        anyhow::bail!("the agents source is never polled")
    }

    fn send(&self, _ctx: &Ctx, op: &str, payload: &Value) -> anyhow::Result<Value> {
        match op {
            // Archiving (app/docs/agents.md › OpenShell sandboxes). An absent sandbox counts as deleted, so a resend is safe.
            crate::agents::openshell::DELETE_OP => {
                let s = |k: &str| payload[k].as_str().unwrap_or_default();
                if s("name").is_empty() {
                    anyhow::bail!("no sandbox name");
                }
                let command = if s("command").is_empty() { "openshell" } else { s("command") };
                let out = crate::agents::openshell::delete_sandbox(command, s("gateway"), s("name")).map_err(|e| anyhow::anyhow!("{e}"))?;
                Ok(json!({ "deleted": s("name"), "said": out.trim() }))
            }
            _ => anyhow::bail!("the agents source doesn't support '{op}'"),
        }
    }
}
