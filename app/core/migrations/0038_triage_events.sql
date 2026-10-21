-- Triage in Incoming, for ignore suggestions (queries/ignore.rs): each
-- dismiss, Gmail trash, and confirm or move ("keep"). `u` deletes the row.
CREATE TABLE triage_events (
  id         INTEGER PRIMARY KEY,
  -- Kept when the item goes (a pruned split part): the count still stands.
  item_id    INTEGER REFERENCES items(id) ON DELETE SET NULL,
  source_id  INTEGER NOT NULL REFERENCES sources(id),
  action     TEXT NOT NULL CHECK (action IN ('dismiss', 'trash', 'keep')),
  at         TEXT NOT NULL,      -- UTC instant
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);
CREATE INDEX triage_events_item ON triage_events (item_id);

-- What an event was about, stored when it happened rather than read off the
-- item later (user, 2026-10-17): a sender, its domain, a mailing list, a
-- channel, a repo or a project.
CREATE TABLE triage_keys (
  event_id   INTEGER NOT NULL REFERENCES triage_events(id) ON DELETE CASCADE,
  kind       TEXT NOT NULL CHECK (kind IN ('from', 'domain', 'list', 'channel', 'repo', 'project')),
  key        TEXT NOT NULL,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  PRIMARY KEY (event_id, kind)
);
CREATE INDEX triage_keys_key ON triage_keys (kind, key);

-- "Not now" on an ignore suggestion, until when (UTC instant).
CREATE TABLE ignore_declines (
  source_id  INTEGER NOT NULL REFERENCES sources(id),
  kind       TEXT NOT NULL,
  key        TEXT NOT NULL,
  until      TEXT NOT NULL,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  PRIMARY KEY (source_id, kind, key)
);
