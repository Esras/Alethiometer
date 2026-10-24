use super::*;

#[test]
fn replaces_whole_and_leaves_no_temp_files() {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("a.json");
    write_atomic(&p, b"one", None).unwrap();
    write_atomic(&p, b"two", None).unwrap();
    assert_eq!(std::fs::read_to_string(&p).unwrap(), "two");
    assert_eq!(std::fs::read_dir(d.path()).unwrap().count(), 1);
}

#[test]
fn write_new_never_replaces_and_keeps_the_mtime() {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("n.md");
    let then = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000);
    write_new(&p, b"first", Some(then)).unwrap();
    assert_eq!(std::fs::metadata(&p).unwrap().modified().unwrap(), then);
    let e = write_new(&p, b"second", None).unwrap_err();
    assert_eq!(e.kind(), std::io::ErrorKind::AlreadyExists);
    assert_eq!(std::fs::read_to_string(&p).unwrap(), "first");
    assert_eq!(std::fs::read_dir(d.path()).unwrap().count(), 1, "no temp file left");
}

#[test]
fn concurrent_write_new_has_one_winner() {
    let d = tempfile::tempdir().unwrap();
    let p = std::sync::Arc::new(d.path().join("r.md"));
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(8));
    let threads: Vec<_> = (0..8)
        .map(|i| {
            let (p, barrier) = (p.clone(), barrier.clone());
            std::thread::spawn(move || {
                barrier.wait();
                write_new(&p, format!("{i}").repeat(10_000).as_bytes(), None).is_ok()
            })
        })
        .collect();
    let wins = threads.into_iter().map(|t| t.join().unwrap()).filter(|&w| w).count();
    assert_eq!(wins, 1);
    let s = std::fs::read_to_string(&*p).unwrap();
    assert_eq!(s.len(), 10_000);
    assert!(
        s.chars().all(|c| c == s.chars().next().unwrap()),
        "one writer's whole file"
    );
}

#[cfg(unix)]
#[test]
fn mode_applies_from_creation() {
    use std::os::unix::fs::PermissionsExt;
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("s.json");
    write_atomic(&p, b"{}", Some(0o600)).unwrap();
    assert_eq!(std::fs::metadata(&p).unwrap().permissions().mode() & 0o777, 0o600);
}

#[test]
fn concurrent_writers_never_leave_a_mix() {
    let d = tempfile::tempdir().unwrap();
    let p = std::sync::Arc::new(d.path().join("c.json"));
    let threads: Vec<_> = (0..8)
        .map(|i| {
            let p = p.clone();
            std::thread::spawn(move || {
                let body = format!("{}", i).repeat(10_000);
                for _ in 0..20 {
                    write_atomic(&p, body.as_bytes(), None).unwrap();
                }
            })
        })
        .collect();
    for t in threads {
        t.join().unwrap();
    }
    let s = std::fs::read_to_string(&*p).unwrap();
    assert_eq!(s.len(), 10_000);
    assert!(
        s.chars().all(|c| c == s.chars().next().unwrap()),
        "one writer's whole file"
    );
}

#[test]
fn write_checked_refuses_a_file_that_changed_and_cleans_up() {
    let dir = tempfile::tempdir().unwrap();
    let abs = dir.path().join("t.md");
    std::fs::write(&abs, "now").unwrap();
    let e = write_checked(&abs, Some("stale"), "new").unwrap_err();
    assert!(e.to_string().contains("changed"), "{e}");
    assert_eq!(std::fs::read_to_string(&abs).unwrap(), "now");
    assert!(!dir.path().join(".t.md.alethiometer-tmp").exists());
    write_checked(&abs, Some("now"), "new").unwrap();
    assert_eq!(std::fs::read_to_string(&abs).unwrap(), "new");
}

#[test]
fn move_to_trash_checks_the_text_and_never_replaces_a_trashed_note() {
    let d = tempfile::tempdir().unwrap();
    let people = d.path().join("People");
    std::fs::create_dir_all(&people).unwrap();
    let a = people.join("Kim.md");
    std::fs::write(&a, "old").unwrap();
    assert!(move_to_trash(d.path(), &a, "something else").is_err());
    assert!(a.exists(), "changed since: left alone");
    assert_eq!(
        move_to_trash(d.path(), &a, "old").unwrap(),
        d.path().join(".trash/Kim.md")
    );
    assert!(!a.exists());
    std::fs::write(&a, "new").unwrap();
    assert_eq!(
        move_to_trash(d.path(), &a, "new").unwrap(),
        d.path().join(".trash/Kim 1.md")
    );
    assert_eq!(std::fs::read_to_string(d.path().join(".trash/Kim.md")).unwrap(), "old");
    assert_eq!(
        std::fs::read_to_string(d.path().join(".trash/Kim 1.md")).unwrap(),
        "new"
    );
}
