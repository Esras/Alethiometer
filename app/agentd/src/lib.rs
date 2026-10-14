//! alethiometer-agentd: hosts Alethiometer's local ACP agents so they outlive
//! the app (a quit, a crash, a dev rebuild).
//!
//! The daemon starts each agent's adapter (claude-agent-acp, pi-acp) as a
//! child process and owns its stdio. It speaks no ACP itself: it passes lines
//! through and numbers everything the agent writes, keeping it in an on-disk
//! log per agent, so the app can reconnect and ask for whatever it missed.
//! The app talks to it over a Unix socket in the data directory (`net`; on
//! Windows too), one JSON message per line (`proto`).

pub mod client;
pub mod net;
pub mod proto;
pub mod server;
mod tree;
