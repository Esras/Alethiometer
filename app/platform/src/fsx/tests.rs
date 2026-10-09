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
