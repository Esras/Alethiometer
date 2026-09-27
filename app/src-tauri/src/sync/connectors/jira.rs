//! Jira connector. TODO: implement.

use crate::sync::{Batch, Connector, Ctx};

pub struct Jira;

impl Connector for Jira {
    fn poll(&self, _ctx: &Ctx) -> anyhow::Result<Batch> {
        anyhow::bail!("Jira connector not built yet")
    }
}
