//! The consistency repair that reads a source again (Settings › Sources).

use super::{apply, Batch, Connector, Ctx};
use crate::domain::consistency::AUTHORED_KINDS;
use crate::error::Result;
use crate::store::Db;
use serde::Serialize;
use std::sync::Mutex;

/// Items read and written per unit of work, so a failure keeps what came before it.
pub const CHUNK: usize = 25;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Refetched {
    /// Items read again and written.
    pub done: usize,
    pub total: usize,
    /// Why it stopped early (a rate limit, the network).
    pub stopped: Option<String>,
}

/// Read the source's items with messages missing an author again, `CHUNK` at a time under its poll lock.
pub fn missing_authors(db: &Db, ctx: &Ctx, connector: &dyn Connector, lock: &Mutex<()>) -> Result<Refetched> {
    let source_id = ctx.source.id;
    let ids: Vec<String> = db.read(|r| {
        Ok(r.consistency()
            .missing_authors(AUTHORED_KINDS)?
            .into_iter()
            .filter(|m| m.source_id == source_id)
            .flat_map(|m| m.items)
            .collect())
    })?;
    refetch(db, ctx, connector, lock, &ids)
}

/// Read `ids` again and write the ones still stored over their stored revision; nothing new is created.
pub fn refetch(db: &Db, ctx: &Ctx, connector: &dyn Connector, lock: &Mutex<()>, ids: &[String]) -> Result<Refetched> {
    let source_id = ctx.source.id;
    let mut out = Refetched {
        done: 0,
        total: ids.len(),
        stopped: None,
    };
    for chunk in ids.chunks(CHUNK) {
        let _polling = lock.lock().unwrap_or_else(|e| e.into_inner());
        let batch = match connector.refetch(ctx, chunk) {
            Ok(b) => b,
            Err(e) => {
                out.stopped = Some(e.to_string());
                break;
            }
        };
        db.write(|uow| {
            let mut items = Vec::new();
            for it in batch.items {
                if uow.items().find(source_id, &it.external_id)?.is_some() {
                    items.push(it);
                }
            }
            let known: Vec<String> = items.iter().map(|i| i.external_id.clone()).collect();
            uow.consistency().forget_revs(source_id, &known)?;
            apply(
                uow,
                ctx.source,
                &Batch {
                    items,
                    ..Default::default()
                },
                ctx.now,
            )
        })?;
        out.done += chunk.len();
    }
    Ok(out)
}
