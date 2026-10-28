//! Settings › Sources › Check consistency: what the checks found, and the repair that needs no network.

use crate::api::{Ctx, Route};
use crate::domain::consistency::AUTHORED_KINDS;
use crate::domain::ports::{Reads, UnitOfWork};
use crate::error::Result;
use serde::Serialize;
use serde_json::{json, Value};

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Finding {
    /// `missing_authors` (repaired by `sync.repair_authors`), `split_senders`, `calendar_leftovers`.
    pub check: &'static str,
    pub source_id: Option<i64>,
    pub source: String,
    pub kind: String,
    pub title: String,
    pub count: usize,
    pub detail: Vec<String>,
    pub repairable: bool,
}

pub fn route(name: &str) -> Option<Route> {
    match name {
        "check" => Some(Route::Read(|r, _: &Ctx, _: &Value| Ok(json!(check(r)?)))),
        "merge_senders" => Some(Route::Write(|uow, _: &Ctx, _: &Value| Ok(json!(merge_senders(uow)?)))),
        _ => None,
    }
}

pub fn check(r: &dyn Reads) -> Result<Vec<Finding>> {
    let sources = r.sources().all()?;
    let name = |id: i64| {
        sources
            .iter()
            .find(|s| s.id == id)
            .map(|s| (s.name.clone(), s.kind.clone()))
    };
    let mut out = Vec::new();
    for m in r.consistency().missing_authors(AUTHORED_KINDS)? {
        let (source, kind) = name(m.source_id).unwrap_or_default();
        let items = m.items.len();
        out.push(Finding {
            check: "missing_authors",
            source_id: Some(m.source_id),
            source,
            kind,
            title: "Messages missing an author".into(),
            count: m.messages as usize,
            detail: vec![format!(
                "on {items} item{}; Repair reads them again from the source",
                if items == 1 { "" } else { "s" }
            )],
            repairable: true,
        });
    }
    let pairs = r.consistency().split_senders()?;
    if !pairs.is_empty() {
        let gmail = sources.iter().find(|s| s.kind == "gmail");
        let noted = pairs.iter().filter(|p| p.noted).count();
        out.push(Finding {
            check: "split_senders",
            source_id: gmail.map(|s| s.id),
            source: gmail.map_or_else(|| "Gmail".into(), |s| s.name.clone()),
            kind: "gmail".into(),
            title: "Same sender on two people".into(),
            count: pairs.len(),
            detail: pairs
                .iter()
                .map(|p| {
                    format!(
                        "{}{}",
                        p.address,
                        if p.noted {
                            " (has a People note, left alone)"
                        } else {
                            ""
                        }
                    )
                })
                .collect(),
            repairable: noted < pairs.len(),
        });
    }
    let mut leftovers = r.consistency().calendar_leftovers()?;
    while let Some(first) = leftovers.first().map(|l| l.source_id) {
        let (mine, rest): (Vec<_>, Vec<_>) = leftovers.into_iter().partition(|l| l.source_id == first);
        leftovers = rest;
        let (source, kind) = name(first).unwrap_or_default();
        out.push(Finding {
            check: "calendar_leftovers",
            source_id: Some(first),
            source,
            kind,
            title: "Calendar leftovers".into(),
            count: mine.len(),
            detail: mine
                .iter()
                .map(|l| format!("{} (item {})", l.title, l.item_id))
                .collect(),
            repairable: false,
        });
    }
    Ok(out)
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MergedSenders {
    pub merged: usize,
    /// Pairs left alone because a People note holds one of them.
    pub refused: usize,
}

/// Each split sender becomes the person holding more of its items (the lower id on a tie).
pub fn merge_senders(uow: &dyn UnitOfWork) -> Result<MergedSenders> {
    let mut out = MergedSenders { merged: 0, refused: 0 };
    for p in uow.reads().consistency().split_senders()? {
        if p.noted {
            out.refused += 1;
            continue;
        }
        let (keep, gone) = match p.bare_items.cmp(&p.named_items) {
            std::cmp::Ordering::Greater => (p.bare, p.named),
            std::cmp::Ordering::Less => (p.named, p.bare),
            std::cmp::Ordering::Equal => (p.bare.min(p.named), p.bare.max(p.named)),
        };
        uow.consistency().merge_person(gone, keep)?;
        out.merged += 1;
    }
    Ok(out)
}

#[cfg(test)]
mod tests;
