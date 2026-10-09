use super::*;

#[test]
fn a_file_that_doesnt_parse_is_never_written_over() {
    let d = tempfile::tempdir().unwrap();
    let s = FileSecrets::new(d.path());
    s.secret_set("a", "1").unwrap();
    s.secret_set("b", "2").unwrap();
    // A hand edit gone wrong (or a crash from before writes were atomic).
    std::fs::write(d.path().join("secrets.json"), "{ \"a\": \"1\", \"b\": ").unwrap();
    assert!(s.secret_set("c", "3").is_err());
    assert_eq!(
        std::fs::read_to_string(d.path().join("secrets.json")).unwrap(),
        "{ \"a\": \"1\", \"b\": ",
        "left for the user to fix"
    );
    assert_eq!(s.secret_get("a"), None);
}

#[test]
fn concurrent_sets_keep_every_key() {
    let d = tempfile::tempdir().unwrap();
    let s = std::sync::Arc::new(FileSecrets::new(d.path()));
    let threads: Vec<_> = (0..8)
        .map(|i| {
            let s = s.clone();
            std::thread::spawn(move || {
                for j in 0..10 {
                    s.secret_set(&format!("k{i}.{j}"), "v").unwrap();
                }
            })
        })
        .collect();
    for t in threads {
        t.join().unwrap();
    }
    for i in 0..8 {
        for j in 0..10 {
            assert_eq!(s.secret_get(&format!("k{i}.{j}")).as_deref(), Some("v"), "k{i}.{j}");
        }
    }
}

#[cfg(unix)]
#[test]
fn the_file_is_private_from_the_start() {
    use std::os::unix::fs::PermissionsExt;
    let d = tempfile::tempdir().unwrap();
    FileSecrets::new(d.path()).secret_set("a", "1").unwrap();
    assert_eq!(
        std::fs::metadata(d.path().join("secrets.json"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
}
