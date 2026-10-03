//! The real binary as its own process: it answers Shutdown before it exits
//! (it once exited first, and the app saw a dropped connection).

#![cfg(unix)]

use agentd::client::Control;
use agentd::proto::{socket_path, Reply, Request};
use std::time::{Duration, Instant};

#[test]
fn the_daemon_process_answers_shutdown_then_exits() {
    let dir = tempfile::tempdir().unwrap();
    let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_alethiometer-agentd")).arg("--dir").arg(dir.path()).spawn().unwrap();
    let sock = socket_path(dir.path());
    let until = Instant::now() + Duration::from_secs(10);
    let c = loop {
        if let Ok(c) = Control::connect(&sock) {
            break c;
        }
        assert!(Instant::now() < until, "the daemon didn't come up");
        std::thread::sleep(Duration::from_millis(20));
    };
    assert!(matches!(c.call(&Request::Hello { protocol: 1 }).unwrap(), Reply::Hello { .. }));
    assert_eq!(c.call(&Request::Shutdown { force: true }).unwrap(), Reply::Ok);
    let status = child.wait().unwrap();
    assert!(status.success());
    assert!(!sock.exists(), "the socket is removed on exit");
}
