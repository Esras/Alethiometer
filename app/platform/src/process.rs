//! Starting helper processes (git, gh, a credential helper) from the app.

use std::ffi::OsStr;
use std::process::{Child, Command};

/// `Command::new(program)` that opens no console window on Windows. The
/// release app is a GUI-subsystem program with no console of its own, so
/// Windows would give every console child (git, gh, cmd) a window of its
/// own that flashes up and closes. Elsewhere this is plain `Command::new`.
pub fn command(program: impl AsRef<OsStr>) -> Command {
    let mut c = Command::new(program);
    hide_window(&mut c);
    c
}

/// Keep `c` from opening a console window on Windows; nothing elsewhere.
pub fn hide_window(c: &mut Command) -> &mut Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(windows_sys::Win32::System::Threading::CREATE_NO_WINDOW);
    }
    c
}

/// Start `c` so it outlives the app: in a process group of its own, so a
/// terminal closing or a dev rebuild doesn't take it down. On Windows it also
/// gets no console window, and leaves any job object the app runs in (a
/// terminal's or a dev tool's, which would end it with the app) when that job
/// allows it.
pub fn spawn_detached(c: &mut Command) -> std::io::Result<Child> {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        c.process_group(0);
        c.spawn()
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        use windows_sys::Win32::Foundation::ERROR_ACCESS_DENIED;
        use windows_sys::Win32::System::Threading::{
            CREATE_BREAKAWAY_FROM_JOB, CREATE_NEW_PROCESS_GROUP, CREATE_NO_WINDOW,
        };
        let flags = CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW;
        c.creation_flags(flags | CREATE_BREAKAWAY_FROM_JOB);
        match c.spawn() {
            // The job doesn't allow breaking away: stay in it.
            Err(e) if e.raw_os_error() == Some(ERROR_ACCESS_DENIED as i32) => {
                c.creation_flags(flags);
                c.spawn()
            }
            r => r,
        }
    }
}
