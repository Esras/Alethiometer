//! A blocking client: a control connection (one reply per request) and a
//! subscription (events until it closes). The app uses one of each.

use crate::proto::{Event, Reply, Request};
use std::collections::BTreeMap;
use std::io::{self, BufRead, BufReader, Write};
use std::path::Path;
use std::sync::Mutex;
use std::time::Duration;

use crate::net::UnixStream;

pub struct Control {
    conn: Mutex<(BufReader<UnixStream>, UnixStream)>,
}

fn bad(e: impl std::fmt::Display) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, e.to_string())
}

impl Control {
    pub fn connect(socket: &Path) -> io::Result<Control> {
        let s = UnixStream::connect(socket)?;
        s.set_read_timeout(Some(Duration::from_secs(15)))?;
        let r = BufReader::new(s.try_clone()?);
        Ok(Control {
            conn: Mutex::new((r, s)),
        })
    }

    pub fn call(&self, req: &Request) -> io::Result<Reply> {
        let mut g = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let mut line = serde_json::to_string(req).map_err(bad)?;
        line.push('\n');
        g.1.write_all(line.as_bytes())?;
        let mut out = String::new();
        if g.0.read_line(&mut out)? == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "the agent host closed the connection",
            ));
        }
        serde_json::from_str(out.trim()).map_err(bad)
    }

    /// `call`, with an `error` reply as an `Err`.
    pub fn ok(&self, req: &Request) -> io::Result<Reply> {
        match self.call(req)? {
            Reply::Error { error } => Err(io::Error::other(error)),
            r => Ok(r),
        }
    }
}

/// Events after `from`, then live ones, handed to `f` until the connection
/// closes (the daemon exited) or `f` returns false.
pub fn subscribe(socket: &Path, from: BTreeMap<String, u64>, mut f: impl FnMut(Event) -> bool) -> io::Result<()> {
    let mut s = UnixStream::connect(socket)?;
    let mut line = serde_json::to_string(&Request::Subscribe { from }).map_err(bad)?;
    line.push('\n');
    s.write_all(line.as_bytes())?;
    let r = BufReader::new(s);
    let mut first = true;
    for l in r.lines() {
        let l = l?;
        match serde_json::from_str::<Reply>(&l).map_err(bad)? {
            Reply::Ev(e) => {
                if !f(e) {
                    return Ok(());
                }
            }
            Reply::Error { error } if first => return Err(io::Error::other(error)),
            _ => {}
        }
        first = false;
    }
    Ok(())
}
