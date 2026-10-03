//! Source sync. Each connector turns a remote API into a `Batch` of normalized
//! records; `apply` writes a batch into SQLite. Connectors never touch the
//! database directly, which keeps them testable with canned HTTP responses.
//!
//! Boundary rules enforced here, not in connectors:
//! - a `personal` source may only produce untitled busy blocks (rule 3);
//! - nothing is ever deleted except a personal source's own busy-block cache,
//!   which is replaced wholesale on each poll (it holds no user content).

pub mod connectors;

/// What `personal` means today, in one line for the Settings page. Keep it in
/// step with `apply` (busy blocks only), the AI context builders (work only),
/// search and Incoming (`s.tag = 'work'`) and the outbox.
pub const PERSONAL_RULES: &str = "Personal sources only store untitled busy blocks: never sent to the AI, never in Incoming or search, nothing written back.";
pub mod http;

use crate::domain::ports::{Reads, UnitOfWork};
use crate::error::{msg, Result};
use crate::clock::Clock;
use crate::time::Ts;
use crate::settings::SourceConfig;
use crate::state::App;
use http::Http;
use rusqlite::Connection;
use serde::Serialize;
use serde_json::Value;

/// A source row plus its settings, handed to a connector.
#[derive(Debug, Clone)]
pub struct Source {
    pub id: i64,
    pub kind: String,
    pub tag: String,
    pub name: String,
    pub config: Value,
    pub cursor: Option<String>,
}

/// What a connector may read while polling or sending.
pub struct Ctx<'a> {
    pub source: &'a Source,
    pub http: &'a dyn Http,
    /// Secrets by name (Keychain on the Mac, 0600 file on Linux).
    pub secret: &'a dyn Fn(&str) -> Option<String>,
    /// Now (respects fake_now).
    pub now: Ts,
    /// For calendar days (all-day events, day-only dues) and local windows.
    pub clock: Clock,
}

impl Ctx<'_> {
    pub fn require_secret(&self, key: &str) -> anyhow::Result<String> {
        (self.secret)(key).ok_or_else(|| anyhow::anyhow!("missing secret '{key}' for source '{}'", self.source.name))
    }

    pub fn config_str(&self, key: &str) -> anyhow::Result<String> {
        self.source
            .config
            .get(key)
            .and_then(Value::as_str)
            .map(str::to_owned)
            .ok_or_else(|| anyhow::anyhow!("source '{}' needs config.{key}", self.source.name))
    }
}

pub use crate::domain::ingest::{Batch, Busy, EventFields, Handle, ItemUpsert, Message, Profile};

/// A connector for one source kind.
pub trait Connector: Send + Sync {
    /// Fetch changes since `ctx.source.cursor`.
    fn poll(&self, ctx: &Ctx) -> anyhow::Result<Batch>;

    /// Perform one outbox write. Return a receipt only once the remote side has
    /// confirmed it; any error leaves the row failed and visible.
    fn send(&self, _ctx: &Ctx, op: &str, _payload: &Value) -> anyhow::Result<Value> {
        anyhow::bail!("this source doesn't support '{op}'")
    }
}

/// Make sure every configured source has a row, and sync its tag/config.
pub fn register(uow: &dyn UnitOfWork, configs: &[SourceConfig]) -> Result<()> {
    for c in configs {
        if c.tag != "work" && c.tag != "personal" {
            return Err(msg(format!("source '{}': tag must be work or personal", c.name)));
        }
        uow.sources().register(&c.kind, &c.name, &c.tag, &c.config.to_string(), c.enabled)?;
    }
    Ok(())
}

/// `register` over a connection (tests and code not moved yet).
pub fn register_sources(conn: &Connection, configs: &[SourceConfig]) -> Result<()> {
    register(&crate::store::over(conn), configs)
}

/// A source as connectors see it.
pub fn source(r: &dyn Reads, id: i64) -> Result<Source> {
    let s = r.sources().get(id)?.ok_or_else(|| msg(format!("no source {id}")))?;
    Ok(Source { id: s.id, kind: s.kind, tag: s.tag, name: s.name, config: serde_json::from_str(&s.config_json).unwrap_or(Value::Null), cursor: s.cursor })
}

/// `source` over a connection (tests and code not moved yet).
pub fn load_source(conn: &Connection, id: i64) -> Result<Source> {
    source(&crate::store::over(conn), id)
}

/// Write a batch, in the caller's unit of work. Returns the number of items
/// that became new signal.
pub fn apply(uow: &dyn UnitOfWork, source: &Source, batch: &Batch, now: Ts) -> Result<usize> {
    if source.tag == "personal" && (!batch.items.is_empty() || !batch.cancelled.is_empty() || !batch.seen.is_empty() || !batch.profiles.is_empty()) {
        return Err(msg(format!(
            "personal source '{}' tried to store items; only busy blocks are allowed",
            source.name
        )));
    }
    let ingest = uow.ingest();
    let mut new_signal = 0;
    let mut touched = std::collections::BTreeSet::new();
    for it in &batch.items {
        let author = it.author.as_ref().map(|h| ingest.handle_person(&source.kind, h)).transpose()?;
        // Split children own some messages of this item (queries::split): those
        // are never re-added here, and a run made only of them isn't new signal.
        let owned = uow.splits().owned_keys(source.id, &it.external_id)?;
        let ats: Vec<String> = it.messages.iter().map(|m| m.at.to_db()).collect();
        let tuples: Vec<(&str, &str, &str)> = it.messages.iter().zip(&ats).map(|(m, at)| (at.as_str(), m.author.as_str(), m.text.as_str())).collect();
        let keys = crate::domain::splits::keys(&it.raw_json, &tuples);
        let skipped = keys.iter().filter(|k| owned.contains(*k)).count();
        let bump = it.bump && (it.messages.is_empty() || skipped < it.messages.len());
        let existing = uow.items().find(source.id, &it.external_id)?;
        if let Some(parent) = &it.attach_to {
            let channel = it.raw_json.get("channel").and_then(Value::as_str);
            if let (None, Some(holder)) = (existing, uow.reads().items().holder(source.id, channel, parent)?) {
                if bump {
                    ingest.mark_read_if_triaged(holder)?;
                }
                for (m, key) in it.messages.iter().zip(&keys).filter(|(_, k)| *k != parent) {
                    // Replies before the run (context) were read; so were ones stored when you triaged.
                    let read = m.context || ingest.reply_read(holder, key, parent)?;
                    ingest.put_reply(holder, m, key, parent, read)?;
                }
                if bump && uow.items().set_seen(holder, false)? {
                    new_signal += 1;
                }
                continue;
            }
        }
        debug_assert!(it.occurred_at != Ts::default(), "{}: occurred_at not set", it.external_id);
        let item_id = match existing {
            None => {
                let id = ingest.insert_item(source.id, it, author)?;
                let series = it.raw_json.get("recurringEventId").and_then(Value::as_str);
                if let Some(k) = series {
                    touched.insert(k.to_string());
                }
                if !series.map_or(Ok(false), |k| ingest.inherit_series(source.id, id, k))? {
                    new_signal += 1;
                }
                id
            }
            Some(id) => {
                if bump {
                    ingest.mark_read_if_triaged(id)?;
                }
                ingest.update_item(id, it, author, bump)?;
                if bump {
                    new_signal += 1;
                }
                id
            }
        };
        if !it.messages.is_empty() {
            let read = ingest.read_keys(item_id)?;
            let rows: Vec<(&Message, &str, bool)> =
                it.messages.iter().zip(&keys).filter(|(_, k)| !owned.contains(*k)).map(|(m, k)| (m, k.as_str(), read.contains(k))).collect();
            ingest.replace_messages(item_id, &rows)?;
            if skipped > 0 {
                uow.splits().recount(item_id, skipped)?;
            }
        }
        if let Some(e) = &it.event {
            ingest.put_event(item_id, source.id, &it.title, e)?;
        } else if it.drop_event {
            ingest.drop_event(item_id)?;
        }
    }
    for ext in &batch.cancelled {
        new_signal += ingest.cancel(source.id, ext)?;
    }
    for p in &batch.profiles {
        ingest.put_profile(p)?;
    }
    for ext in &batch.seen {
        ingest.mark_seen(source.id, ext)?;
    }
    for k in &touched {
        ingest.collapse_series(source.id, k, now)?;
    }
    if let Some(busy) = &batch.busy {
        ingest.replace_busy(source.id, busy)?;
    }
    ingest.finish(source.id, batch.cursor.as_deref())?;
    Ok(new_signal)
}

/// `apply` over a connection, in its own transaction (tests and code not moved yet).
pub fn apply_in(conn: &Connection, source: &Source, batch: &Batch, now: Ts) -> Result<usize> {
    let tx = conn.unchecked_transaction()?;
    let n = apply(&crate::store::over(&tx), source, batch, now)?;
    tx.commit()?;
    Ok(n)
}

static RUNNING: std::sync::Mutex<Vec<i64>> = std::sync::Mutex::new(Vec::new());

struct Running(i64);

impl Running {
    fn start(id: i64) -> Running {
        RUNNING.lock().unwrap_or_else(|e| e.into_inner()).push(id);
        Running(id)
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        let mut r = RUNNING.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(i) = r.iter().position(|id| *id == self.0) {
            r.remove(i);
        }
    }
}

fn is_running(id: i64) -> bool {
    RUNNING.lock().unwrap_or_else(|e| e.into_inner()).contains(&id)
}

/// Poll one source now: the poll outside any database access, then the
/// batch and its follow-ups in one unit of work. Errors are stored on the
/// source row and returned.
pub fn sync_source(app: &App, source_id: i64) -> Result<usize> {
    let _running = Running::start(source_id);
    let mut source = app.store().read(|r| source(r, source_id))?;
    if source.kind == "slack" {
        source.cursor = app.store().read(|r| connectors::slack::close_triaged(r, &source))?;
    }
    let connector = connectors::for_kind(&source.kind).ok_or_else(|| msg(format!("no connector for '{}'", source.kind)))?;
    let http = http::UreqHttp::default();
    let secret = |k: &str| app.platform.secret_get(k);
    let ctx = Ctx { source: &source, http: &http, secret: &secret, now: app.clock.now(), clock: app.clock.clone() };
    let result = connector.poll(&ctx).map_err(|e| msg(e.to_string())).and_then(|batch| {
        let n = app.store().write(|uow| {
            let n = apply(uow, &source, &batch, ctx.now)?;
            // Follow-ups in the same unit of work (topic links, closing vanished tasks).
            match source.kind.as_str() {
                "obsidian" => Ok(n.saturating_sub(connectors::obsidian::after_apply(uow, &source, &batch)?)),
                // GitHub: items from a repo linked to exactly one topic become suggested links.
                "github" => {
                    uow.links().suggest_from_repos(source.id)?;
                    Ok(n)
                }
                _ => Ok(n),
            }
        })?;
        // People notes changed: read them now the batch is in.
        if source.kind == "obsidian" {
            if let Some(vault) = connectors::obsidian::people_touched(&source, &batch) {
                crate::queries::people::rescan_vault(app, &vault)?;
            }
        }
        Ok(n)
    });
    if let Err(e) = &result {
        app.store().write(|uow| uow.sources().set_error(source_id, &e.to_string()))?;
    }
    result
}

/// New signals may land in Unsorted; ask the small model for topic suggestions.
/// A no-op (with a status) when AI isn't configured; never fails the sync.
/// Re-stamps `last_sync_at` afterwards so the UI reloads once the suggestions have landed.
pub fn suggest_after_sync(app: &App, source_id: i64) {
    let _running = crate::ai::suggest::Running::start();
    if let Err(e) = crate::ai::call(app, "suggest_topics", &serde_json::json!({ "limit": 10 })) {
        eprintln!("suggest_topics: {e}");
    }
    if let Err(e) = crate::ai::call(app, "estimate_todos", &serde_json::json!({ "limit": 10 })) {
        eprintln!("estimate_todos: {e}");
    }
    let _ = app.store().write(|uow| uow.sources().touch_synced(source_id));
}

/// Each source's loops carry the generation they were started with and stop
/// once it moves on: restarting a source (its settings changed) or disabling it
/// bumps the generation, so only that connector restarts.
static GENERATIONS: std::sync::Mutex<std::collections::BTreeMap<i64, u64>> = std::sync::Mutex::new(std::collections::BTreeMap::new());

pub(crate) fn bump_generation(id: i64) -> u64 {
    let mut g = GENERATIONS.lock().unwrap_or_else(|e| e.into_inner());
    let n = g.entry(id).or_insert(0);
    *n += 1;
    *n
}

/// Whether loops started at generation `gen` for source `id` should keep going.
pub fn alive(id: i64, gen: u64) -> bool {
    GENERATIONS.lock().unwrap_or_else(|e| e.into_inner()).get(&id).copied() == Some(gen)
}

/// Sleep up to `d`, waking early (false) once the source's loops should stop.
pub fn sleep_alive(id: i64, gen: u64, d: std::time::Duration) -> bool {
    let until = std::time::Instant::now() + d;
    loop {
        if !alive(id, gen) {
            return false;
        }
        let left = until.saturating_duration_since(std::time::Instant::now());
        if left.is_zero() {
            return true;
        }
        std::thread::sleep(left.min(std::time::Duration::from_secs(1)));
    }
}

static THREADS: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Whether background sync runs in this process (not in tests or one-off tools).
pub fn running_threads() -> bool {
    THREADS.load(std::sync::atomic::Ordering::Relaxed)
}

/// Background polling for every enabled source. One thread per source.
pub fn start(app: std::sync::Arc<App>) {
    THREADS.store(true, std::sync::atomic::Ordering::Relaxed);
    let Ok(ids) = app.store().read(|r| Ok(r.sources().all()?.into_iter().map(|s| s.id).collect::<Vec<i64>>())) else { return };
    for id in ids {
        start_source(app.clone(), id);
    }
}

/// Stop a source's loops (if any) and start them again from its current
/// settings. A source that's disabled or no longer in settings stays stopped.
pub fn start_source(app: std::sync::Arc<App>, id: i64) {
    let gen = bump_generation(id);
    let Ok(source) = app.store().read(|r| source(r, id)) else { return };
    let settings = app.settings();
    let Some(cfg) = settings.sources.iter().find(|s| s.kind == source.kind && s.name == source.name) else { return };
    // Glean is search-only: queried on demand, never polled. Scion Hubs feed the Agents tab (`agents::scion`).
    if !cfg.enabled || cfg.kind == "glean" || cfg.kind == "scion" {
        return;
    }
    let name = cfg.name.clone();
    let poll = std::time::Duration::from_secs(cfg.poll_secs.max(30));
    // Slack: live Socket Mode nudges on top of polling (no-op without an app-level token).
    if cfg.kind == "slack" {
        connectors::slack::socket::start(app.clone(), id, gen);
    }
    // The vault runs its own loop: file watcher plus a short poll.
    if cfg.kind == "obsidian" {
        connectors::obsidian::run(app, id, gen, name, poll);
        return;
    }
    std::thread::Builder::new()
        .name(format!("sync-{name}"))
        .spawn(move || {
            while alive(id, gen) {
                match sync_source(&app, id) {
                    Ok(_) => suggest_after_sync(&app, id),
                    Err(e) => eprintln!("sync {name}: {e}"),
                }
                if !sleep_alive(id, gen, poll) {
                    break;
                }
            }
        })
        .ok();
}

/// Stop a source's loops (removed or disabled in settings).
pub fn stop_source(id: i64) {
    bump_generation(id);
}

#[derive(Serialize)]
pub struct SourceStatus {
    pub id: i64,
    pub kind: String,
    pub tag: String,
    pub name: String,
    pub last_sync_at: Option<Ts>,
    pub last_error: Option<String>,
    pub syncing: bool,
    /// Slack only: whether the live socket is connected (else it polls).
    pub live: Option<bool>,
}

pub fn route(name: &str) -> Option<crate::api::Route> {
    match name {
        // Scion Hubs and agents show their state in the Agents tab, not here.
        "status" => Some(crate::api::Route::Read(|r, _, _| {
            let rows: Vec<SourceStatus> = r
                .sources()
                .all()?
                .into_iter()
                .filter(|s| s.kind != "scion" && s.kind != "agents")
                .map(|s| SourceStatus {
                    syncing: is_running(s.id),
                    live: if s.kind == "slack" { connectors::slack::socket::connected(s.id) } else { None },
                    id: s.id,
                    kind: s.kind,
                    tag: s.tag,
                    name: s.name,
                    last_sync_at: s.last_sync_at,
                    last_error: s.last_error,
                })
                .collect();
            Ok(serde_json::json!(rows))
        })),
        _ => None,
    }
}

pub fn call(app: &App, name: &str, args: &Value) -> Result<Value> {
    match name {
        "now" => {
            let id = crate::queries::arg_i64(args, "source_id")?;
            let n = sync_source(app, id)?;
            Ok(serde_json::json!({ "new_signals": n }))
        }
        "suggest" => {
            suggest_after_sync(app, crate::queries::arg_i64(args, "source_id")?);
            Ok(Value::Null)
        }
        _ => Err(crate::api::unknown(name)),
    }
}

/// `apply` at the fixtures' "now" (Sep 28 2026, 10:52 UTC).
#[cfg(test)]
pub fn apply_now(conn: &Connection, source: &Source, batch: &Batch) -> Result<usize> {
    apply_in(conn, source, batch, crate::time::t("2026-09-28T10:52:00"))
}

/// A clock at `now` (an RFC 3339 instant) in UTC, for connector tests.
#[cfg(test)]
pub fn test_clock(now: &str) -> Clock {
    Clock::fixed(Ts::parse(now).expect("an RFC 3339 instant"), jiff::tz::TimeZone::UTC)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup(tag: &str) -> (Connection, Source) {
        let conn = crate::db::open_in_memory().unwrap();
        let cfg = SourceConfig {
            kind: "jira".into(),
            tag: tag.into(),
            name: "Work Jira".into(),
            enabled: true,
            poll_secs: 300,
            config: serde_json::json!({}),
        };
        register_sources(&conn, &[cfg]).unwrap();
        let id: i64 = conn.query_row("SELECT id FROM sources", [], |r| r.get(0)).unwrap();
        let src = load_source(&conn, id).unwrap();
        (conn, src)
    }

    fn issue(bump: bool) -> ItemUpsert {
        ItemUpsert {
            external_id: "PLAT-1".into(),
            kind: "issue".into(),
            title: "Fix retries".into(),
            occurred_at: crate::time::t("2026-09-28T09:00:00"),
            status: "open".into(),
            author: Some(Handle { handle: "acc-1".into(), display_name: "Jordan".into() }),
            bump,
            ..Default::default()
        }
    }

    #[test]
    fn upsert_marks_new_then_only_bumps_resurface() {
        let (conn, src) = setup("work");
        let b = Batch { items: vec![issue(false)], cursor: Some("c1".into()), ..Default::default() };
        assert_eq!(apply_in(&conn, &src, &b, crate::time::t("2026-09-28T10:52:00")).unwrap(), 1);
        conn.execute("UPDATE items SET seen = 1", []).unwrap();
        assert_eq!(apply_in(&conn, &src, &b, crate::time::t("2026-09-28T10:52:00")).unwrap(), 0);
        let seen: i64 = conn.query_row("SELECT seen FROM items", [], |r| r.get(0)).unwrap();
        assert_eq!(seen, 1);
        let b2 = Batch { items: vec![issue(true)], ..Default::default() };
        assert_eq!(apply_in(&conn, &src, &b2, crate::time::t("2026-09-28T10:52:00")).unwrap(), 1);
        let (seen, n, cursor): (i64, i64, String) = conn
            .query_row("SELECT seen, (SELECT count(*) FROM items), (SELECT cursor FROM sources) FROM items", [], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            })
            .unwrap();
        assert_eq!((seen, n, cursor.as_str()), (0, 1, "c1"));
        // One person per handle.
        let people: i64 = conn.query_row("SELECT count(*) FROM people", [], |r| r.get(0)).unwrap();
        assert_eq!(people, 1);
    }

    #[test]
    fn profiles_are_stored_newest_wins() {
        let conn = crate::db::open_in_memory().unwrap();
        conn.execute("INSERT INTO sources (kind, tag, name) VALUES ('slack', 'work', 'Slack')", []).unwrap();
        let src = load_source(&conn, 1).unwrap();
        let p = |email: Option<&str>| Profile { kind: "slack".into(), account: "U1".into(), real_name: Some("Ann Lee".into()), email: email.map(str::to_owned), is_bot: false };
        apply_now(&conn, &src, &Batch { profiles: vec![p(Some("Ann@X.com"))], ..Default::default() }).unwrap();
        apply_now(&conn, &src, &Batch { profiles: vec![p(Some("ann.lee@x.com"))], ..Default::default() }).unwrap();
        let got: (String, String) = conn.query_row("SELECT real_name, email FROM account_profiles", [], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
        assert_eq!(got, ("Ann Lee".into(), "ann.lee@x.com".into()));
    }

    #[test]
    fn personal_source_cannot_store_items() {
        let (conn, src) = setup("personal");
        let b = Batch { items: vec![issue(false)], ..Default::default() };
        assert!(apply_in(&conn, &src, &b, crate::time::t("2026-09-28T10:52:00")).is_err());
        let n: i64 = conn.query_row("SELECT count(*) FROM items", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 0);
    }

    #[test]
    fn busy_blocks_replace_and_keep_buffer_overrides() {
        let (conn, src) = setup("personal");
        let busy = |s: &str, e: &str| Busy { start: s.into(), end: e.into() };
        let b = Batch { busy: Some(vec![busy("2026-09-28T12:00:00", "2026-09-28T13:00:00")]), ..Default::default() };
        apply_in(&conn, &src, &b, crate::time::t("2026-09-28T10:52:00")).unwrap();
        conn.execute("UPDATE events SET buffer_min = 25", []).unwrap();
        let b = Batch {
            busy: Some(vec![busy("2026-09-28T12:00:00", "2026-09-28T13:00:00"), busy("2026-09-28T17:30:00", "2026-09-28T18:00:00")]),
            ..Default::default()
        };
        apply_in(&conn, &src, &b, crate::time::t("2026-09-28T10:52:00")).unwrap();
        let rows: Vec<(String, Option<i64>, Option<String>)> = conn
            .prepare("SELECT start, buffer_min, title FROM events ORDER BY start")
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].1, Some(25));
        assert!(rows.iter().all(|r| r.2.is_none()));
    }

    #[test]
    fn recurring_series_shows_once_and_new_instances_inherit_its_topic() {
        let (conn, src) = setup("work");
        let inst = |id: &str, at: &str| ItemUpsert {
            external_id: id.into(),
            kind: "event".into(),
            title: "Standup".into(),
            occurred_at: at.into(),
            status: "open".into(),
            raw_json: serde_json::json!({ "recurringEventId": "S" }),
            bump: true,
            ..Default::default()
        };
        let b = Batch {
            items: vec![inst("past", "2000-01-01T09:00:00"), inst("feb", "2099-02-01T09:00:00"), inst("jan", "2099-01-01T09:00:00")],
            ..Default::default()
        };
        apply_in(&conn, &src, &b, crate::time::t("2026-09-28T10:52:00")).unwrap();
        let unseen = |c: &Connection| -> Vec<String> {
            c.prepare("SELECT external_id FROM items WHERE seen = 0").unwrap().query_map([], |r| r.get(0)).unwrap().map(|r| r.unwrap()).collect()
        };
        assert_eq!(unseen(&conn), ["jan"]);

        conn.execute("INSERT INTO topics (id, name, category) VALUES (1, 'Standups', 'work')", []).unwrap();
        let jan: i64 = conn.query_row("SELECT id FROM items WHERE external_id = 'jan'", [], |r| r.get(0)).unwrap();
        crate::queries::incoming::move_to(&crate::store::over(&conn), crate::time::Ts::new(jiff::Timestamp::now()), jan, 1, None).unwrap();
        let linked = |c: &Connection| -> i64 {
            c.query_row("SELECT count(*) FROM topic_links WHERE topic_id = 1 AND state = 'confirmed'", [], |r| r.get(0)).unwrap()
        };
        assert_eq!(linked(&conn), 3);

        let b = Batch { items: vec![inst("mar", "2099-03-01T09:00:00")], ..Default::default() };
        assert_eq!(apply_in(&conn, &src, &b, crate::time::t("2026-09-28T10:52:00")).unwrap(), 0);
        assert!(unseen(&conn).is_empty());
        assert_eq!(linked(&conn), 4);
    }

    #[test]
    fn bad_tag_is_rejected() {
        let conn = crate::db::open_in_memory().unwrap();
        let cfg = SourceConfig {
            kind: "jira".into(),
            tag: "home".into(),
            name: "x".into(),
            enabled: true,
            poll_secs: 300,
            config: serde_json::json!({}),
        };
        assert!(register_sources(&conn, &[cfg]).is_err());
    }
}
