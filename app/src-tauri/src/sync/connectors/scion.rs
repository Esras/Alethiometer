//! Scion Hub writes through the outbox. The Hub is read by the Agents tab
//! (`agents::scion`), never by the sync workers.

use crate::agents::scion::api::Client;
use crate::settings::sources::ScionConfig;
use crate::sync::{Batch, Connector, Ctx};
use serde_json::Value;

pub struct Scion;

impl Connector for Scion {
    fn poll(&self, _ctx: &Ctx) -> anyhow::Result<Batch> {
        anyhow::bail!("Scion Hubs are read by the Agents tab")
    }

    fn send(&self, ctx: &Ctx, op: &str, payload: &Value) -> anyhow::Result<Value> {
        let cfg = ScionConfig::from_json(&ctx.source.config)?;
        let client = Client {
            http: ctx.http,
            base: cfg.hub_url.clone(),
            token: ctx.require_secret(&cfg.token_secret)?.trim().to_owned(),
        };
        match op {
            // Archiving (app/docs/agents.md › Archive).
            "scion.delete_agent" => {
                let agent = payload["agent"]
                    .as_str()
                    .filter(|a| !a.is_empty())
                    .ok_or_else(|| anyhow::anyhow!("no agent id"))?;
                client.delete_agent(agent)
            }
            _ => anyhow::bail!("Scion doesn't support '{op}'"),
        }
    }
}
