-- Glean (search-only). Calls per day, against the source's calls_per_day.
CREATE TABLE glean_calls (
  day  TEXT PRIMARY KEY,           -- YYYY-MM-DD, local
  n    INTEGER NOT NULL DEFAULT 0
);

-- Pins and rejects on hydrated search results, per topic. A pinned result is
-- always kept for that topic (and first); a rejected one is always dropped.
-- `key` is `item:<id>` for a local item, else the result's URL.
CREATE TABLE search_feedback (
  topic_id  INTEGER NOT NULL REFERENCES topics(id),
  key       TEXT NOT NULL,
  verdict   TEXT NOT NULL,         -- pin | reject
  title     TEXT,
  url       TEXT,
  source    TEXT,
  at        TEXT NOT NULL DEFAULT (datetime('now', 'localtime')),
  PRIMARY KEY (topic_id, key)
);
