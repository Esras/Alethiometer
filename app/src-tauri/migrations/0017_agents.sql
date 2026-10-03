-- Agents tab (app/docs/agents.md). Local groups, sessions and transcripts,
-- as JSON (`agents::model`); Scion state comes from the Hub and isn't kept.
CREATE TABLE agent_groups (
  id    TEXT PRIMARY KEY,          -- local:<n>
  data  TEXT NOT NULL
);

CREATE TABLE agent_sessions (
  id    TEXT PRIMARY KEY,          -- acp-<hex>
  data  TEXT NOT NULL
);

CREATE TABLE agent_rows (
  thread  TEXT NOT NULL,           -- s:<session id>
  id      INTEGER NOT NULL,
  data    TEXT NOT NULL,
  PRIMARY KEY (thread, id)
);

-- Agent colours per group, in order of creation (local and Scion).
CREATE TABLE agent_colors (
  grp   TEXT NOT NULL,
  name  TEXT NOT NULL,
  idx   INTEGER NOT NULL,
  PRIMARY KEY (grp, name)
);
