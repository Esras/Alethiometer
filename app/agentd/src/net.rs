//! The socket the app and the daemon talk over: a Unix socket, on Windows
//! too (AF_UNIX, Windows 10 1803 and later, through `uds_windows`), so both
//! sides have one implementation everywhere.

#[cfg(unix)]
pub use std::os::unix::net::{UnixListener, UnixStream};
#[cfg(windows)]
pub use uds_windows::{UnixListener, UnixStream};
