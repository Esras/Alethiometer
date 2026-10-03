-- Column defaults that wrote local time or SQLite's space-separated form now
-- write a UTC instant (`crate::time::Ts`). SQLite can't change a default in
-- place, so each table is rebuilt (foreign keys are off while migrating).

CREATE TABLE topic_links_new (
  item_id     INTEGER NOT NULL REFERENCES items(id),
  topic_id    INTEGER NOT NULL REFERENCES topics(id),
  state       TEXT NOT NULL CHECK (state IN ('suggested', 'confirmed', 'wrong')),
  confidence  REAL,
  reason      TEXT,
  updated_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  PRIMARY KEY (item_id, topic_id)
);
INSERT INTO topic_links_new SELECT item_id, topic_id, state, confidence, reason, updated_at FROM topic_links;
DROP TABLE topic_links;
ALTER TABLE topic_links_new RENAME TO topic_links;
CREATE INDEX topic_links_topic ON topic_links(topic_id, state);

CREATE TABLE priorities_new (
  item_id       INTEGER PRIMARY KEY REFERENCES items(id),
  quadrant      TEXT NOT NULL CHECK (quadrant IN ('now', 'make_room', 'quick', 'can_wait')),
  score         REAL NOT NULL,
  reasons_json  TEXT NOT NULL DEFAULT '[]', -- [{label, weight: red|yellow|green}]
  updated_at    TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);
INSERT INTO priorities_new SELECT item_id, quadrant, score, reasons_json, updated_at FROM priorities;
DROP TABLE priorities;
ALTER TABLE priorities_new RENAME TO priorities;

CREATE TABLE outbox_new (
  id            INTEGER PRIMARY KEY,
  source_id     INTEGER NOT NULL REFERENCES sources(id),
  op            TEXT NOT NULL,
  payload_json  TEXT NOT NULL,
  state         TEXT NOT NULL DEFAULT 'pending' CHECK (state IN ('pending', 'sent', 'confirmed', 'failed')),
  error         TEXT,
  created_at    TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  confirmed_at  TEXT,
  item_id       INTEGER REFERENCES items(id),
  attempts      INTEGER NOT NULL DEFAULT 0,
  receipt_json  TEXT,
  sent_at       TEXT
);
INSERT INTO outbox_new (id, source_id, op, payload_json, state, error, created_at, confirmed_at, item_id, attempts, receipt_json, sent_at)
  SELECT id, source_id, op, payload_json, state, error, created_at, confirmed_at, item_id, attempts, receipt_json, sent_at FROM outbox;
DROP TABLE outbox;
ALTER TABLE outbox_new RENAME TO outbox;
CREATE INDEX outbox_state ON outbox(state);
CREATE INDEX outbox_item ON outbox(item_id) WHERE item_id IS NOT NULL;

CREATE TABLE ai_usage_new (
  id            INTEGER PRIMARY KEY,
  at            TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  tier          TEXT NOT NULL,
  model         TEXT NOT NULL,
  input_tokens  INTEGER NOT NULL,
  output_tokens INTEGER NOT NULL
);
INSERT INTO ai_usage_new SELECT id, at, tier, model, input_tokens, output_tokens FROM ai_usage;
DROP TABLE ai_usage;
ALTER TABLE ai_usage_new RENAME TO ai_usage;

CREATE TABLE held_todos_new (
  item_id    INTEGER PRIMARY KEY REFERENCES items(id),
  target     TEXT NOT NULL CHECK (target IN ('local', 'jira')),
  text       TEXT NOT NULL,
  due        TEXT,               -- a calendar day, YYYY-MM-DD
  state      TEXT NOT NULL DEFAULT 'held' CHECK (state IN ('held', 'discarded')),
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);
INSERT INTO held_todos_new SELECT item_id, target, text, due, state, created_at FROM held_todos;
DROP TABLE held_todos;
ALTER TABLE held_todos_new RENAME TO held_todos;

CREATE TABLE todo_estimates_new (
  item_id    INTEGER PRIMARY KEY REFERENCES items(id),
  minutes    INTEGER NOT NULL CHECK (minutes > 0),
  by         TEXT NOT NULL CHECK (by IN ('ai', 'you')),
  updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);
INSERT INTO todo_estimates_new SELECT item_id, minutes, by, updated_at FROM todo_estimates;
DROP TABLE todo_estimates;
ALTER TABLE todo_estimates_new RENAME TO todo_estimates;

CREATE TABLE account_profiles_new (
  kind        TEXT NOT NULL,             -- slack
  account     TEXT NOT NULL,             -- the source's id (Slack user id)
  real_name   TEXT,
  email       TEXT,                      -- lowercased
  updated_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  is_bot      INTEGER NOT NULL DEFAULT 0,
  PRIMARY KEY (kind, account)
);
INSERT INTO account_profiles_new SELECT kind, account, real_name, email, updated_at, is_bot FROM account_profiles;
DROP TABLE account_profiles;
ALTER TABLE account_profiles_new RENAME TO account_profiles;

CREATE TABLE embed_meta_new (
  kind   TEXT NOT NULL,              -- item | topic
  id     INTEGER NOT NULL,
  model  TEXT NOT NULL,
  hash   TEXT NOT NULL,              -- of the text that was embedded
  at     TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  PRIMARY KEY (kind, id)
);
INSERT INTO embed_meta_new SELECT kind, id, model, hash, at FROM embed_meta;
DROP TABLE embed_meta;
ALTER TABLE embed_meta_new RENAME TO embed_meta;

CREATE TABLE search_feedback_new (
  topic_id  INTEGER NOT NULL REFERENCES topics(id),
  key       TEXT NOT NULL,
  verdict   TEXT NOT NULL,         -- pin | reject
  title     TEXT,
  url       TEXT,
  source    TEXT,
  at        TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  PRIMARY KEY (topic_id, key)
);
INSERT INTO search_feedback_new SELECT topic_id, key, verdict, title, url, source, at FROM search_feedback;
DROP TABLE search_feedback;
ALTER TABLE search_feedback_new RENAME TO search_feedback;
