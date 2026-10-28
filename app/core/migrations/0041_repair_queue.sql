-- Items a consistency repair asked the source's own sync to read again, at its pace.
CREATE TABLE repair_queue (
  source_id   INTEGER NOT NULL REFERENCES sources(id),
  external_id TEXT NOT NULL,
  created_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  updated_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  PRIMARY KEY (source_id, external_id)
);
