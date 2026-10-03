//! Live queries (`app/docs/db-plan.md`, Phase 4). The UI subscribes to a
//! read command with its arguments; the core runs it and notes every table
//! it read (`store::Db::read_traced`). After each commit it re-runs only the
//! subscriptions that read a table the commit changed, and a result that
//! differs goes to the client's waiting `live.wait`, a long poll that works
//! the same on the desktop and in the browser.
//!
//! - Commands: `live.subscribe {client, cmd, args}` → `{id, result, seq}`,
//!   `live.unsubscribe {id}`, `live.wait {client, since, ids}` → `{seq,
//!   changed: [{id, result, seq}], gone}` once something changed after
//!   `since` (or empty after `WAIT`). A result's `seq` orders it against
//!   others for the same subscription; `gone` lists the `ids` the core no
//!   longer has (dropped while the client was idle), to subscribe again.
//! - Every subscription is also re-run each `REFRESH`: results that depend on
//!   the time, and writes no hook here saw (another app on the same data dir).
//! - A client that stops waiting for `IDLE` has its subscriptions dropped.

use crate::api::{Ctx, Route};
use crate::error::{msg, Result};
use crate::state::App;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

const WAIT: Duration = Duration::from_secs(25);
const REFRESH: Duration = Duration::from_secs(60);
const IDLE: Duration = Duration::from_secs(120);
/// A table name meaning "all of them": settings changed.
const ALL: &str = "*";

#[derive(Default)]
pub struct Live {
    state: Mutex<State>,
    cv: Condvar,
    started: std::sync::OnceLock<()>,
}

#[derive(Default)]
struct State {
    next_id: u64,
    /// Bumped whenever a result changes.
    seq: u64,
    /// Handed to each run as it starts: a run that started later read a
    /// snapshot at least as new, so an older run never overwrites its result.
    tickets: u64,
    subs: BTreeMap<u64, Sub>,
}

struct Sub {
    client: String,
    cmd: String,
    args: Value,
    /// What its last stored run read (`ALL` until the first is stored).
    tables: BTreeSet<String>,
    result: Value,
    /// The ticket of the run that stored `result`.
    ticket: u64,
    /// `seq` when the result last changed.
    changed: u64,
    seen: Instant,
}

impl Live {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Re-run every subscription soon (settings changed).
    pub fn refresh_all(app: &App) {
        app.store().changes().touch([ALL.to_owned()]);
    }
}

fn read_fn(cmd: &str) -> Result<fn(&dyn crate::domain::ports::Reads, &Ctx, &Value) -> Result<Value>> {
    match crate::api::route_of(cmd) {
        Some(Route::Read(f)) => Ok(f),
        _ => Err(msg(format!("'{cmd}' can't be a live query (only routed reads can)"))),
    }
}

/// Run a read command, with the tables it read.
fn run(app: &App, cmd: &str, args: &Value) -> Result<(Value, BTreeSet<String>)> {
    let f = read_fn(cmd)?;
    let settings = app.settings();
    let ctx = Ctx { clock: &app.clock, settings: &settings };
    app.store().read_traced(|r| f(r, &ctx, args))
}

pub fn subscribe(app: &App, client: &str, cmd: &str, args: Value) -> Result<Value> {
    read_fn(cmd)?;
    start(app);
    // Registered before its first run, matching every table: a commit that
    // lands meanwhile re-runs it, and the newer run's result is kept.
    let (id, seq) = {
        let mut st = app.live.lock();
        st.next_id += 1;
        let id = st.next_id;
        let sub = Sub {
            client: client.to_owned(),
            cmd: cmd.to_owned(),
            args: args.clone(),
            tables: BTreeSet::from([ALL.to_owned()]),
            result: Value::Null,
            ticket: 0,
            changed: 0,
            seen: Instant::now(),
        };
        st.subs.insert(id, sub);
        (id, st.seq)
    };
    if let Err(e) = run_and_store(app, id, cmd, &args) {
        app.live.lock().subs.remove(&id);
        return Err(e);
    }
    let st = app.live.lock();
    let (result, seq) = st.subs.get(&id).map(|s| (s.result.clone(), s.changed)).unwrap_or((Value::Null, seq));
    Ok(json!({ "id": id, "result": result, "seq": seq }))
}

/// Run a subscription's command and keep the result unless a newer run
/// already did. Returns whether the stored result changed.
fn run_and_store(app: &App, id: u64, cmd: &str, args: &Value) -> Result<bool> {
    let ticket = {
        let mut st = app.live.lock();
        st.tickets += 1;
        st.tickets
    };
    // No lock held while it runs: a query takes as long as it takes.
    let (result, read) = run(app, cmd, args)?;
    let mut st = app.live.lock();
    let changed = match st.subs.get(&id) {
        Some(s) if ticket > s.ticket => s.result != result,
        _ => return Ok(false),
    };
    if changed {
        st.seq += 1;
    }
    let seq = st.seq;
    let s = st.subs.get_mut(&id).expect("checked above");
    s.ticket = ticket;
    s.tables = read;
    if changed {
        s.result = result;
        s.changed = seq;
    }
    Ok(changed)
}

pub fn unsubscribe(app: &App, id: u64) {
    app.live.lock().subs.remove(&id);
}

/// The client's subscriptions whose result changed after `since`, once there
/// are any (or none after `timeout`), and the sequence to wait from next.
/// Returns at once if any of `ids` is gone.
pub fn wait(app: &App, client: &str, since: u64, ids: &[u64], timeout: Duration) -> Value {
    let live = &app.live;
    let deadline = Instant::now() + timeout;
    let mut st = live.lock();
    loop {
        let now = Instant::now();
        let mut changed = vec![];
        for (id, s) in st.subs.iter_mut().filter(|(_, s)| s.client == client) {
            s.seen = now;
            if s.changed > since {
                changed.push(json!({ "id": id, "result": s.result, "seq": s.changed }));
            }
        }
        let gone: Vec<u64> = ids.iter().copied().filter(|id| !st.subs.contains_key(id)).collect();
        if !changed.is_empty() || !gone.is_empty() || now >= deadline {
            return json!({ "seq": st.seq, "changed": changed, "gone": gone });
        }
        st = live.cv.wait_timeout(st, deadline - now).unwrap_or_else(|e| e.into_inner()).0;
    }
}

/// Re-run what the changed `tables` (all subscriptions when empty or `ALL`)
/// could have changed; push what did. Returns how many results changed.
fn rerun(app: &App, tables: &BTreeSet<String>) -> usize {
    let everything = tables.is_empty() || tables.contains(ALL);
    let due: Vec<(u64, String, Value)> = {
        let mut st = app.live.lock();
        st.subs.retain(|_, s| s.seen.elapsed() < IDLE);
        st.subs
            .iter()
            .filter(|(_, s)| everything || !s.tables.is_disjoint(tables))
            .map(|(id, s)| (*id, s.cmd.clone(), s.args.clone()))
            .collect()
    };
    let mut n = 0;
    for (id, cmd, args) in due {
        match run_and_store(app, id, &cmd, &args) {
            Ok(true) => n += 1,
            Ok(false) => {}
            Err(e) => eprintln!("live {cmd}: {e}"),
        }
    }
    if n > 0 {
        app.live.cv.notify_all();
    }
    n
}

/// The re-runner, once per process.
fn start(app: &App) {
    let Some(arc) = app.arc() else { return };
    app.live.started.get_or_init(move || {
        let spawned = std::thread::Builder::new().name("live".into()).spawn(move || loop {
            let tables = arc.store().changes().take(REFRESH);
            rerun(&arc, &tables);
        });
        if let Err(e) = spawned {
            eprintln!("live queries: couldn't start: {e}");
        }
    });
}

pub fn call(app: &App, name: &str, args: &Value) -> Result<Value> {
    let client = || crate::queries::arg_str(args, "client");
    match name {
        "subscribe" => subscribe(app, &client()?, &crate::queries::arg_str(args, "cmd")?, args.get("args").cloned().unwrap_or(json!({}))),
        "unsubscribe" => {
            unsubscribe(app, crate::queries::arg_i64(args, "id")? as u64);
            Ok(json!({ "ok": true }))
        }
        "wait" => {
            let since = args.get("since").and_then(Value::as_u64).unwrap_or(0);
            let ids: Vec<u64> = args.get("ids").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_u64).collect()).unwrap_or_default();
            Ok(wait(app, &client()?, since, &ids, WAIT))
        }
        _ => Err(crate::api::unknown(name)),
    }
}

#[cfg(test)]
pub(crate) fn rerun_now(app: &App, tables: &BTreeSet<String>) -> usize {
    rerun(app, tables)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::msg;

    fn app() -> (App, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let app = App::from_parts(crate::db::open_in_memory().unwrap(), Default::default(), dir.path()).unwrap();
        (app, dir)
    }

    fn ticket(app: &App, id: u64) -> u64 {
        app.live.lock().subs[&id].ticket
    }

    /// What the last commits changed, taken.
    fn changed(app: &App) -> BTreeSet<String> {
        app.store().changes().take(Duration::ZERO)
    }

    #[test]
    fn a_write_reruns_only_what_read_the_tables_it_changed() {
        let (app, _d) = app();
        changed(&app);
        let topics = subscribe(&app, "c1", "topics.list", json!({})).unwrap();
        let strip = subscribe(&app, "c1", "daystrip.get", json!({})).unwrap();
        let (t_id, s_id) = (topics["id"].as_u64().unwrap(), strip["id"].as_u64().unwrap());
        let since = topics["seq"].as_u64().unwrap();
        assert!(app.live.lock().subs[&t_id].tables.contains("topics"), "the tables it read are known");
        let (t0, s0) = (ticket(&app, t_id), ticket(&app, s_id));

        // A Notes line touches neither query's tables.
        let today = app.clock.today();
        app.store().write(|uow| uow.note_lines().insert(today, 0, "note", "x", &Default::default())).unwrap();
        let tables = changed(&app);
        assert_eq!(tables, BTreeSet::from(["notes_lines".to_owned()]));
        assert_eq!(rerun_now(&app, &tables), 0);
        assert_eq!((ticket(&app, t_id), ticket(&app, s_id)), (t0, s0), "neither re-ran");

        // Renaming a topic re-runs the topics list, and its new result is pushed.
        let id: i64 = app.store().read(|r| Ok(r.topics().list(false, app.clock.now(), app.clock.now())?[0].id)).unwrap();
        app.store().write(|uow| uow.topics().rename(id, "Renamed live")).unwrap();
        let tables = changed(&app);
        assert!(tables.contains("topics"));
        // The day strip reads topics only to name today's blocks: it re-runs
        // exactly when its last run read them.
        let strip_reads_topics = app.live.lock().subs[&s_id].tables.contains("topics");
        assert!(rerun_now(&app, &tables) >= 1);
        assert!(ticket(&app, t_id) > t0);
        assert_eq!(ticket(&app, s_id) > s0, strip_reads_topics);
        let w = wait(&app, "c1", since, &[], Duration::ZERO);
        let pushed = w["changed"].as_array().unwrap();
        let topics_now = pushed.iter().find(|c| c["id"].as_u64() == Some(t_id)).unwrap();
        assert!(topics_now["result"].to_string().contains("Renamed live"));
        // Nothing new since then.
        assert!(wait(&app, "c1", w["seq"].as_u64().unwrap(), &[t_id, s_id], Duration::ZERO)["changed"].as_array().unwrap().is_empty());
    }

    #[test]
    fn a_rolled_back_write_announces_nothing() {
        let (app, _d) = app();
        changed(&app);
        let today = app.clock.today();
        let r: Result<()> = app.store().write(|uow| {
            uow.note_lines().insert(today, 0, "note", "x", &Default::default())?;
            Err(msg("changed my mind"))
        });
        assert!(r.is_err());
        assert!(changed(&app).is_empty());
    }

    #[test]
    fn a_dropped_subscription_is_reported_gone() {
        let (app, _d) = app();
        let id = subscribe(&app, "c1", "topics.list", json!({})).unwrap()["id"].as_u64().unwrap();
        unsubscribe(&app, id);
        let w = wait(&app, "c1", 0, &[id], Duration::from_secs(5));
        assert_eq!(w["gone"], json!([id]));
    }

    #[test]
    fn deleting_every_row_is_announced() {
        // SQLite's update hook misses rows a whole-table DELETE truncates.
        let (app, _d) = app();
        changed(&app);
        app.store().write(|uow| uow.categories().replace(&[])).unwrap();
        assert!(changed(&app).contains("categories"));
    }

    #[test]
    fn only_routed_reads_can_be_live() {
        let (app, _d) = app();
        assert!(subscribe(&app, "c1", "incoming.confirm", json!({})).is_err());
        assert!(subscribe(&app, "c1", "notes.page", json!({})).is_err());
    }
}
