//! GitHub connector. TODO: implement.

use crate::sync::{Batch, Connector, Ctx};

pub struct GitHub;

impl Connector for GitHub {
    fn poll(&self, _ctx: &Ctx) -> anyhow::Result<Batch> {
        anyhow::bail!("GitHub connector not built yet")
    }
}
