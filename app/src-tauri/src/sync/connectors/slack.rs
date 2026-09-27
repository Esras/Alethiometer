//! Slack connector. TODO: implement.

use crate::sync::{Batch, Connector, Ctx};

pub struct Slack;

impl Connector for Slack {
    fn poll(&self, _ctx: &Ctx) -> anyhow::Result<Batch> {
        anyhow::bail!("Slack connector not built yet")
    }
}
