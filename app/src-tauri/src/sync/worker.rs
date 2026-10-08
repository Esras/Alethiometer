//! One worker thread per source owns its polls (`app/docs/db-plan.md`, Phase 2).
//!
//! - The worker polls on its source's interval. "Sync now", the Slack
//!   socket's nudges and the vault watcher send it a message instead of
//!   polling themselves, and messages queued during a poll are answered by
//!   the next one.
//! - Restarting a source (its settings changed) stops the old worker and
//!   joins it before the new one polls, so two polls of one source never
//!   overlap and a slow old poll can't write its cursor after a newer one.
//! - `sync_source` also takes the source's poll lock, so a direct call (a
//!   test, a source with no worker) can't overlap a worker's poll either.

use super::Connector;
use crate::error::{msg, Result};
use crate::state::App;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// A poll's outcome as the caller that asked for it gets it.
type Reply = Sender<std::result::Result<usize, String>>;

enum Msg {
    /// Poll now; answer `Reply` if someone waits for it.
    Poll(Option<Reply>),
    Stop,
}

struct Worker {
    gen: u64,
    tx: Sender<Msg>,
    thread: Option<JoinHandle<()>>,
}

/// The app's source workers, and one poll lock per source (held for a
/// whole poll: fetch and apply).
#[derive(Default)]
pub struct Workers {
    running: Mutex<BTreeMap<i64, Worker>>,
    locks: Mutex<BTreeMap<i64, Arc<Mutex<()>>>>,
}

fn workers(app: &App) -> std::sync::MutexGuard<'_, BTreeMap<i64, Worker>> {
    app.workers.running.lock().unwrap_or_else(|e| e.into_inner())
}

pub(super) fn poll_lock(app: &App, id: i64) -> Arc<Mutex<()>> {
    app.workers
        .locks
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .entry(id)
        .or_default()
        .clone()
}

/// How a source is polled: on a timer, and what follows a timed poll.
pub(super) struct Plan {
    pub name: String,
    pub every: Duration,
    /// Ask the small model for topic suggestions after each timed poll.
    pub suggest: bool,
}

/// Stop the source's worker (if any) and start a new one with `plan`; `None`
/// leaves it stopped. Returns at once: the new worker joins the old one first.
pub(super) fn restart(app: Arc<App>, id: i64, gen: u64, plan: Option<Plan>) {
    let Some(plan) = plan else {
        // Nothing to start; the old worker finishes its poll and exits on its own.
        stop(&app, id);
        return;
    };
    let (tx, rx) = mpsc::channel();
    let old_thread = workers(&app)
        .insert(id, Worker { gen, tx, thread: None })
        .and_then(|w| {
            let _ = w.tx.send(Msg::Stop);
            w.thread
        });
    let name = plan.name.clone();
    let worker_app = app.clone();
    let spawned = std::thread::Builder::new().name(format!("sync-{name}")).spawn(move || {
        if let Some(t) = old_thread {
            let _ = t.join();
        }
        run(&worker_app, id, plan, rx);
    });
    match spawned {
        Ok(t) => {
            if let Some(w) = workers(&app).get_mut(&id).filter(|w| w.gen == gen) {
                w.thread = Some(t);
            }
        }
        Err(e) => eprintln!("sync {name}: couldn't start its worker: {e}"),
    }
}

/// Stop the source's worker, if it has one. It finishes a poll in progress.
pub(super) fn stop(app: &App, id: i64) {
    if let Some(w) = workers(app).remove(&id) {
        let _ = w.tx.send(Msg::Stop);
    }
}

/// Ask the source's worker to poll soon. False when it has no worker.
pub fn nudge(app: &App, id: i64) -> bool {
    workers(app)
        .get(&id)
        .is_some_and(|w| w.tx.send(Msg::Poll(None)).is_ok())
}

/// Poll now and wait for the outcome: through the source's worker when it
/// has one (queued behind a poll in progress), else directly.
pub fn poll_now(app: &App, id: i64) -> Result<usize> {
    let (tx, rx) = mpsc::channel();
    let sent = workers(app)
        .get(&id)
        .is_some_and(|w| w.tx.send(Msg::Poll(Some(tx))).is_ok());
    if !sent {
        return super::sync_source(app, id);
    }
    match rx.recv() {
        Ok(r) => r.map_err(msg),
        // The worker stopped before it got to it (restarted or disabled).
        Err(_) => super::sync_source(app, id),
    }
}

fn run(app: &App, id: i64, plan: Plan, rx: Receiver<Msg>) {
    let suggesting = Arc::new(AtomicBool::new(false));
    let mut next = Instant::now();
    loop {
        let mut replies = vec![];
        let mut stop = false;
        let timed = match rx.recv_timeout(next.saturating_duration_since(Instant::now())) {
            Ok(Msg::Poll(r)) => {
                replies.extend(r);
                false
            }
            Ok(Msg::Stop) | Err(RecvTimeoutError::Disconnected) => return,
            Err(RecvTimeoutError::Timeout) => true,
        };
        // Everything queued meanwhile is answered by this one poll.
        while let Ok(m) = rx.try_recv() {
            match m {
                Msg::Poll(r) => replies.extend(r),
                Msg::Stop => stop = true,
            }
        }
        let result = super::sync_source(app, id);
        if let Err(e) = &result {
            eprintln!("sync {}: {e}", plan.name);
        }
        for r in replies {
            let _ = r.send(result.as_ref().map(|n| *n).map_err(|e| e.to_string()));
        }
        if timed && result.is_ok() && plan.suggest {
            suggest_in_background(app, id, &suggesting);
        }
        if stop {
            return;
        }
        next = Instant::now() + plan.every;
    }
}

/// Topic suggestions after a poll, off the worker so the next poll (or a
/// "Sync now") doesn't wait on the model. One at a time per source.
fn suggest_in_background(app: &App, id: i64, busy: &Arc<AtomicBool>) {
    let Some(app) = app.arc() else { return };
    if busy.swap(true, Ordering::SeqCst) {
        return;
    }
    let flag = busy.clone();
    let spawned = std::thread::Builder::new()
        .name(format!("suggest-{id}"))
        .spawn(move || {
            super::suggest_after_sync(&app, id);
            flag.store(false, Ordering::SeqCst);
        });
    if spawned.is_err() {
        busy.store(false, Ordering::SeqCst);
    }
}

/// Connectors tests register by kind (`for_kind` checks here first).
#[cfg(test)]
pub(super) static TEST_CONNECTORS: Mutex<BTreeMap<String, Arc<dyn Connector>>> = Mutex::new(BTreeMap::new());

#[cfg(test)]
pub(super) fn test_connector(kind: &str) -> Option<Arc<dyn Connector>> {
    TEST_CONNECTORS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(kind)
        .cloned()
}

#[cfg(not(test))]
pub(super) fn test_connector(_kind: &str) -> Option<Arc<dyn Connector>> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync::{Batch, Ctx};
    use std::sync::atomic::AtomicUsize;

    /// Records how many polls run at once.
    #[derive(Default)]
    struct Probe {
        inside: AtomicUsize,
        most: AtomicUsize,
        polls: AtomicUsize,
    }

    impl Connector for Probe {
        fn poll(&self, _ctx: &Ctx) -> anyhow::Result<Batch> {
            let n = self.inside.fetch_add(1, Ordering::SeqCst) + 1;
            self.most.fetch_max(n, Ordering::SeqCst);
            std::thread::sleep(Duration::from_millis(20));
            self.inside.fetch_sub(1, Ordering::SeqCst);
            self.polls.fetch_add(1, Ordering::SeqCst);
            Ok(Batch::default())
        }
    }

    fn app_with(kind: &str) -> (Arc<App>, i64, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let settings = crate::settings::Settings {
            demo_data: false,
            sources: vec![crate::settings::SourceConfig {
                kind: kind.into(),
                tag: "work".into(),
                name: "Probe".into(),
                enabled: true,
                poll_secs: 30,
                config: serde_json::json!({}),
            }],
            ..Default::default()
        };
        let app = App::shared(App::from_parts(crate::db::open_in_memory().unwrap(), settings, dir.path()).unwrap());
        let id = app.store().read(|r| r.sources().find(kind, "Probe")).unwrap().unwrap();
        (app, id, dir)
    }

    #[test]
    fn sync_now_nudges_restarts_and_direct_calls_never_overlap() {
        let probe = Arc::new(Probe::default());
        TEST_CONNECTORS
            .lock()
            .unwrap()
            .insert("probe-overlap".into(), probe.clone());
        let (app, id, _dir) = app_with("probe-overlap");
        crate::sync::start_source(app.clone(), id);
        let mut threads = vec![];
        for i in 0..24 {
            let app = app.clone();
            threads.push(std::thread::spawn(move || match i % 4 {
                0 => {
                    poll_now(&app, id).unwrap();
                }
                1 => {
                    nudge(&app, id);
                }
                2 => {
                    crate::sync::sync_source(&app, id).unwrap();
                }
                _ => crate::sync::start_source(app.clone(), id),
            }));
        }
        for t in threads {
            t.join().unwrap();
        }
        // Let the last worker answer what was queued.
        poll_now(&app, id).unwrap();
        crate::sync::stop_source(&app, id);
        assert_eq!(
            probe.most.load(Ordering::SeqCst),
            1,
            "two polls of one source ran at once"
        );
        assert!(
            probe.polls.load(Ordering::SeqCst) >= 7,
            "sync-now and direct calls each got a poll"
        );
    }

    #[test]
    fn queued_requests_share_the_next_poll() {
        let probe = Arc::new(Probe::default());
        TEST_CONNECTORS
            .lock()
            .unwrap()
            .insert("probe-coalesce".into(), probe.clone());
        let (app, id, _dir) = app_with("probe-coalesce");
        crate::sync::start_source(app.clone(), id);
        poll_now(&app, id).unwrap();
        let before = probe.polls.load(Ordering::SeqCst);
        let threads: Vec<_> = (0..10)
            .map(|_| {
                let app = app.clone();
                std::thread::spawn(move || poll_now(&app, id).unwrap())
            })
            .collect();
        for t in threads {
            t.join().unwrap();
        }
        crate::sync::stop_source(&app, id);
        let polls = probe.polls.load(Ordering::SeqCst) - before;
        assert!((1..10).contains(&polls), "10 requests answered by {polls} polls");
    }
}
