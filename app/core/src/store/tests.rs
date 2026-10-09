use super::*;
use crate::error::msg;

fn file_db() -> (Db, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let db = Db::new(schema::open(&dir.path().join("t.sqlite")).unwrap()).unwrap();
    (db, dir)
}

/// A life block (id 1) with a 10-minute buffer.
fn life_block(db: &Db) {
    db.legacy()
        .execute_batch(
            "INSERT INTO sources (id, kind, tag, name) VALUES (1, 'protoncal', 'personal', 'Life');
             INSERT INTO events (id, source_id, start, end, category, is_life_busy, buffer_min)
             VALUES (1, 1, '2026-10-03T17:00:00.000Z', '2026-10-03T18:00:00.000Z', 'life', 1, 10);",
        )
        .unwrap();
}

fn buffer(r: &dyn Reads) -> Option<i64> {
    let day = (
        crate::time::t("2026-10-03T00:00:00"),
        crate::time::t("2026-10-04T00:00:00"),
    );
    r.day_strip().blocks(day.0, day.1).unwrap()[0].buffer_min
}

#[test]
fn an_error_or_a_panic_rolls_the_whole_unit_of_work_back() {
    let (db, _dir) = file_db();
    life_block(&db);
    let r: Result<()> = db.write(|uow| {
        uow.events().set_buffer(1, 20)?;
        uow.events().set_buffer(1, 30)?;
        Err(msg("failed after two writes"))
    });
    assert!(r.is_err());
    assert_eq!(db.read(|r| Ok(buffer(r))).unwrap(), Some(10));
    let p = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = db.write(|uow| -> Result<()> {
            uow.events().set_buffer(1, 20)?;
            panic!("mid-action");
        });
    }));
    assert!(p.is_err());
    assert_eq!(db.read(|r| Ok(buffer(r))).unwrap(), Some(10), "the panic rolled back");
    db.write(|uow| uow.events().set_buffer(1, 25)).unwrap();
    assert_eq!(
        db.read(|r| Ok(buffer(r))).unwrap(),
        Some(25),
        "and the writer still works"
    );
}

#[test]
fn a_long_read_runs_while_a_write_commits_and_keeps_its_snapshot() {
    let (db, _dir) = file_db();
    life_block(&db);
    let db = std::sync::Arc::new(db);
    let (started_tx, started) = std::sync::mpsc::channel();
    let (go_tx, go) = std::sync::mpsc::channel::<()>();
    let reader = {
        let db = db.clone();
        std::thread::spawn(move || {
            db.read(|r| {
                let before = buffer(r);
                started_tx.send(()).unwrap();
                go.recv().unwrap(); // the write commits meanwhile
                Ok((before, buffer(r)))
            })
            .unwrap()
        })
    };
    started.recv().unwrap();
    let t = Instant::now();
    db.write(|uow| uow.events().set_buffer(1, 45)).unwrap();
    assert!(
        t.elapsed() < Duration::from_secs(1),
        "the write didn't wait for the read"
    );
    go_tx.send(()).unwrap();
    assert_eq!(
        reader.join().unwrap(),
        (Some(10), Some(10)),
        "one snapshot for the whole read"
    );
    assert_eq!(db.read(|r| Ok(buffer(r))).unwrap(), Some(45));
}

/// The Glean quota is checked and counted in one unit of work: many
/// searches at once take exactly the calls left.
#[test]
fn concurrent_glean_calls_never_pass_the_daily_limit() {
    let (db, _dir) = file_db();
    let db = std::sync::Arc::new(db);
    let day = crate::time::Date::constant(2026, 10, 3);
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(12));
    let threads: Vec<_> = (0..12)
        .map(|_| {
            let (db, barrier) = (db.clone(), barrier.clone());
            std::thread::spawn(move || {
                barrier.wait();
                db.write(|uow| uow.usage().take_glean_call(day, 5)).unwrap()
            })
        })
        .collect();
    let took = threads.into_iter().map(|t| t.join().unwrap()).filter(|t| *t).count();
    assert_eq!(took, 5);
    assert_eq!(db.read(|r| r.usage().glean_calls(day)).unwrap(), 5);
}

#[test]
#[should_panic(expected = "already held on this thread")]
fn the_writer_taken_twice_on_one_thread_is_a_bug_not_a_hang() {
    let (db, _dir) = file_db();
    let _ = db.write(|_| {
        let _inner = db.legacy();
        Ok(())
    });
}
