-- Local agents run in the agent host (app/agentd). Per agent: how far into
-- its event log the app has read, and the turn state (open rows, tool calls,
-- prompts and approvals in flight) needed to pick up after a restart.
CREATE TABLE agent_hosts (
  id    TEXT PRIMARY KEY,          -- acp-<hex>, the session id
  seq   INTEGER NOT NULL,
  turn  TEXT NOT NULL              -- JSON (agents::acp::Turn)
);
