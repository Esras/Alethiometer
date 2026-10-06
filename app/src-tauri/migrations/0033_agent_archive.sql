-- Archived agents (app/docs/agents.md › Archive). Archiving moves a group or
-- session out of agent_groups / agent_sessions into here, as it was then, so
-- it never loads into the tree again. Its rows stay in agent_rows (a Scion
-- thread is copied there from the Hub first). No unarchive.
CREATE TABLE agent_archive (
  id           TEXT PRIMARY KEY,   -- the group or session id
  kind         TEXT NOT NULL CHECK (kind IN ('group', 'session')),
  source       TEXT NOT NULL,      -- acp | scion
  -- Where it sat in the tree, as names (its group may be gone by now).
  project      TEXT NOT NULL,
  group_name   TEXT NOT NULL,
  group_id     TEXT,               -- a session's group; NULL for a group or an Unsorted session
  archived_at  TEXT NOT NULL,      -- UTC instant
  data         TEXT NOT NULL       -- the Group or Session as JSON (agents::model)
);
CREATE INDEX agent_archive_group ON agent_archive (group_id);

-- "Not now" on a done group's nudge, until when (UTC instant).
CREATE TABLE agent_nudges (
  grp    TEXT PRIMARY KEY,
  until  TEXT NOT NULL
);
