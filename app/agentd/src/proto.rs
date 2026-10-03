//! The socket protocol: one JSON object per line, `op` says which. Every
//! request gets exactly one reply; a `subscribe` connection then carries
//! events (`ev`) until it closes. Bump `PROTOCOL` on any change the other
//! side can't ignore.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub const PROTOCOL: u32 = 1;
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Request {
    Hello { protocol: u32 },
    /// Start an agent. `id` is the app's session id; it names the log too.
    /// `after`: number events from here on (the app's place in this id's log), so
    /// an agent started again under the same id never reuses a number.
    Spawn {
        id: String,
        cmd: String,
        args: Vec<String>,
        env: BTreeMap<String, String>,
        cwd: String,
        #[serde(default)]
        after: u64,
    },
    /// A line for the agent's stdin (an ACP message).
    Write { id: String, line: String },
    /// Stop the agent (its whole process group).
    Kill { id: String },
    List,
    /// Events after `from[id]` for every agent (0 or missing: from the start), then live ones.
    Subscribe { from: BTreeMap<String, u64> },
    /// Drop an exited agent's log.
    Forget { id: String },
    /// Exit. Refused while agents run unless `force`.
    Shutdown { force: bool },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Reply {
    Hello { protocol: u32, version: String, pid: u32, started_ms: u64 },
    Spawned { id: String, pid: u32 },
    Hosts { hosts: Vec<HostInfo> },
    Ok,
    Error { error: String },
    Ev(Event),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HostInfo {
    pub id: String,
    pub pid: u32,
    pub alive: bool,
    pub exit_code: Option<i32>,
    /// The last event's number.
    pub seq: u64,
    pub started_ms: u64,
    pub cmd: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// A line the agent wrote to stdout.
    Out,
    /// A line on stderr.
    Err,
    /// The agent exited (`code` is None when it was killed by a signal, or
    /// when the daemon itself went away while it ran).
    Exit,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Event {
    pub id: String,
    pub seq: u64,
    pub at_ms: u64,
    pub kind: Kind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<i32>,
}

/// The daemon's socket for a data directory: `<dir>/agentd.sock`, or one in
/// the temp directory when that path is too long for a Unix socket (~104 bytes).
pub fn socket_path(dir: &Path) -> PathBuf {
    let p = dir.join("agentd.sock");
    if p.as_os_str().len() <= 100 {
        return p;
    }
    let mut h: u64 = 0xcbf29ce484222325;
    for b in dir.as_os_str().as_encoded_bytes() {
        h = (h ^ *b as u64).wrapping_mul(0x100000001b3);
    }
    std::env::temp_dir().join(format!("alethiometer-agentd-{:08x}.sock", h as u32))
}

/// Where the daemon keeps its state: logs, pid, its own log.
pub fn state_dir(dir: &Path) -> PathBuf {
    dir.join("agentd")
}

pub fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

/// Ids name log files, so only a safe set of characters.
pub fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 128 && id.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '~'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wire_shapes() {
        let r = serde_json::to_string(&Request::Write { id: "acp-1".into(), line: "{}".into() }).unwrap();
        assert_eq!(r, r#"{"op":"write","id":"acp-1","line":"{}"}"#);
        let e = Reply::Ev(Event { id: "acp-1".into(), seq: 3, at_ms: 5, kind: Kind::Out, line: Some("x".into()), code: None });
        let s = serde_json::to_string(&e).unwrap();
        assert_eq!(s, r#"{"op":"ev","id":"acp-1","seq":3,"at_ms":5,"kind":"out","line":"x"}"#);
        assert_eq!(serde_json::from_str::<Reply>(&s).unwrap(), e);
    }

    #[test]
    fn long_dirs_get_a_short_socket() {
        assert_eq!(socket_path(Path::new("/d")), PathBuf::from("/d/agentd.sock"));
        let long = Path::new("/tmp").join("x".repeat(120));
        let s = socket_path(&long);
        assert!(s.as_os_str().len() <= 100 && s.starts_with(std::env::temp_dir()));
        assert_eq!(s, socket_path(&long), "stable for the same directory");
    }

    #[test]
    fn ids() {
        assert!(valid_id("acp-0a1b2c3d") && valid_id("acp-1~ab12"));
        assert!(!valid_id("../x") && !valid_id("") && !valid_id("a/b"));
    }
}
