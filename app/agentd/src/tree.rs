//! An agent's process tree, so Kill reaches what the adapter starts
//! (npx → node, cmd → node): a process group on Unix, a job object on
//! Windows.

use std::io;
use std::process::{ChildStderr, ChildStdin, ChildStdout, Command};
use std::sync::{Mutex, MutexGuard};

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// What `spawn` hands back: the tree and the agent's pipes.
pub struct Spawned {
    pub tree: Tree,
    pub stdin: Option<ChildStdin>,
    pub stdout: Option<ChildStdout>,
    pub stderr: Option<ChildStderr>,
}

#[cfg(unix)]
mod imp {
    use super::*;
    use std::os::unix::process::CommandExt;
    use std::process::Child;

    pub struct Tree {
        pid: u32,
        child: Mutex<Option<Child>>,
    }

    pub fn spawn(mut c: Command) -> io::Result<Spawned> {
        // Its own process group, so signals reach what it starts.
        c.process_group(0);
        let mut child = c.spawn()?;
        Ok(Spawned {
            stdin: child.stdin.take(),
            stdout: child.stdout.take(),
            stderr: child.stderr.take(),
            tree: Tree {
                pid: child.id(),
                child: Mutex::new(Some(child)),
            },
        })
    }

    impl Tree {
        pub fn pid(&self) -> u32 {
            self.pid
        }

        /// Wait for the agent to exit (once; later calls return None at once).
        /// `code` is None when it was killed by a signal.
        pub fn wait(&self) -> Option<i32> {
            let child = lock(&self.child).take();
            child.and_then(|mut c| c.wait().ok()).and_then(|s| s.code())
        }

        /// Ask the group to stop (SIGTERM).
        pub fn terminate(&self) {
            signal_group(self.pid, libc::SIGTERM);
        }

        /// End the whole group now (SIGKILL).
        pub fn kill(&self) {
            signal_group(self.pid, libc::SIGKILL);
        }
    }

    /// Signals the agent's process group (it leads one: `process_group(0)`).
    /// Not through `kill(1)`: procps-ng reads `kill -TERM -<pid>` as
    /// `kill(-1, SIGTERM)`, every process the user owns. Never 0 or 1 either:
    /// `killpg` would reach this daemon's group or every process.
    pub fn signal_group(pid: u32, sig: libc::c_int) -> bool {
        match libc::pid_t::try_from(pid) {
            Ok(p) if p > 1 => unsafe { libc::killpg(p, sig) == 0 },
            _ => false,
        }
    }
}

#[cfg(windows)]
mod imp {
    use super::*;
    use process_wrap::std::{ChildWrapper, CommandWrap, CreationFlags, JobObject};
    use std::time::Duration;
    use windows::Win32::System::Threading::{CREATE_NEW_PROCESS_GROUP, CREATE_NO_WINDOW};

    pub struct Tree {
        pid: u32,
        child: Mutex<Box<dyn ChildWrapper>>,
    }

    /// In a job object of its own, assigned before its first instruction
    /// runs (process-wrap starts it suspended), so nothing it starts escapes.
    pub fn spawn(c: Command) -> io::Result<Spawned> {
        let mut w = CommandWrap::from(c);
        // No console window; Ctrl+C in the daemon's console doesn't reach it.
        w.wrap(CreationFlags(CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP));
        w.wrap(JobObject);
        let mut child = w.spawn()?;
        Ok(Spawned {
            stdin: child.stdin().take(),
            stdout: child.stdout().take(),
            stderr: child.stderr().take(),
            tree: Tree {
                pid: child.id(),
                child: Mutex::new(child),
            },
        })
    }

    impl Tree {
        pub fn pid(&self) -> u32 {
            self.pid
        }

        /// Wait for the agent to exit. Polled, so `kill` can take the lock
        /// meanwhile; a blocking wait would hold it until the exit.
        pub fn wait(&self) -> Option<i32> {
            loop {
                match lock(&self.child).try_wait() {
                    Ok(Some(s)) => return s.code(),
                    Ok(None) => {}
                    Err(_) => return None,
                }
                std::thread::sleep(Duration::from_millis(100));
            }
        }

        /// Windows has no SIGTERM; closing stdin is the polite stop.
        pub fn terminate(&self) {}

        /// End every process in the job now.
        pub fn kill(&self) {
            let _ = lock(&self.child).start_kill();
        }
    }
}

pub use imp::*;
