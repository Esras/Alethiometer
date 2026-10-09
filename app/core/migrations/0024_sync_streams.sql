-- How each stream of a source (a Slack conversation, a thread) last went.
-- A poll reads its streams one by one: one that fails is recorded here and
-- retried next time, and the others go on (app/docs/db-plan.md, Phase 2).
CREATE TABLE sync_streams (
  source_id INTEGER NOT NULL REFERENCES sources(id),
  stream    TEXT NOT NULL,
  -- The last failure, cleared when it's read again.
  error     TEXT,
  -- Failures in a row.
  failures  INTEGER NOT NULL DEFAULT 0,
  -- Last read without an error.
  synced_at TEXT,
  PRIMARY KEY (source_id, stream)
);
