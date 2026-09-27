//! ProtonCalendar connector. TODO: implement.

use crate::sync::{Batch, Connector, Ctx};

pub struct ProtonCalendar;

impl Connector for ProtonCalendar {
    fn poll(&self, _ctx: &Ctx) -> anyhow::Result<Batch> {
        anyhow::bail!("ProtonCalendar connector not built yet")
    }
}
