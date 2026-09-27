//! GoogleCalendar connector. TODO: implement.

use crate::sync::{Batch, Connector, Ctx};

pub struct GoogleCalendar;

impl Connector for GoogleCalendar {
    fn poll(&self, _ctx: &Ctx) -> anyhow::Result<Batch> {
        anyhow::bail!("GoogleCalendar connector not built yet")
    }
}
