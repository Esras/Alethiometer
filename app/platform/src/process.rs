//! Starting helper processes (git, gh, a credential helper) from the app.

use std::ffi::OsStr;
use std::process::Command;

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
