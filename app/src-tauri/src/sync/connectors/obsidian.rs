//! Obsidian connector. TODO: implement.

use crate::sync::{Batch, Connector, Ctx};

pub struct Obsidian;

impl Connector for Obsidian {
    fn poll(&self, _ctx: &Ctx) -> anyhow::Result<Batch> {
        anyhow::bail!("Obsidian connector not built yet")
    }
}
