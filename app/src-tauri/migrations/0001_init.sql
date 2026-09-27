-- Helm schema v1. Plain SQL; applied in order by db::migrate using PRAGMA user_version.
-- Every source is tagged work|personal (boundary rule 1). Personal sources only ever
-- produce untitled busy blocks (rule 3).

CREATE TABLE sources (
  id            INTEGER PRIMARY KEY,
  kind          TEXT NOT NULL,            -- gcal | slack | jira | github | obsidian | protoncal | gmail | marvin | sheets | notion
  tag           TEXT NOT NULL CHECK (tag IN ('work', 'personal')),
  name          TEXT NOT NULL,
  config_json   TEXT NOT NULL DEFAULT '{}',
  cursor        TEXT,
  last_sync_at  TEXT
);

CREATE TABLE people (
  id            INTEGER PRIMARY KEY,
  display_name  TEXT NOT NULL,
  role          TEXT                       -- short line shown under the name
);

CREATE TABLE person_handles (
  person_id     INTEGER NOT NULL REFERENCES people(id),
  source_kind   TEXT NOT NULL,
  handle        TEXT NOT NULL,
  PRIMARY KEY (source_kind, handle)
);

CREATE TABLE items (
  id               INTEGER PRIMARY KEY,
  source_id        INTEGER NOT NULL REFERENCES sources(id),
  external_id      TEXT NOT NULL,
  kind             TEXT NOT NULL,          -- message | thread | issue | pr | event | note | email | doc | task | sheet
  title            TEXT NOT NULL,
  body             TEXT NOT NULL DEFAULT '',
  summary          TEXT,                   -- one-line context shown under the title
  url              TEXT,
  author_person_id INTEGER REFERENCES people(id),
  occurred_at      TEXT NOT NULL,          -- RFC3339, local time
  ended_at         TEXT,                   -- for threads spanning a range
  due_at           TEXT,
  status           TEXT NOT NULL DEFAULT 'open',  -- open | done | new
  msg_count        INTEGER,
  owner            TEXT,                   -- who holds it ("you", "Kim")
  seen             INTEGER NOT NULL DEFAULT 0,    -- 0 = new signal for Incoming
  raw_json         TEXT NOT NULL DEFAULT '{}',
  UNIQUE (source_id, external_id)
);

CREATE TABLE item_messages (                -- expanded context for a signal (e.g. Slack msgs since last look)
  id         INTEGER PRIMARY KEY,
  item_id    INTEGER NOT NULL REFERENCES items(id),
  at         TEXT NOT NULL,
  author     TEXT NOT NULL,
  text       TEXT NOT NULL
);

CREATE VIRTUAL TABLE items_fts USING fts5(title, body, content='items', content_rowid='id');
CREATE TRIGGER items_ai AFTER INSERT ON items BEGIN
  INSERT INTO items_fts(rowid, title, body) VALUES (new.id, new.title, new.body);
END;
CREATE TRIGGER items_ad AFTER DELETE ON items BEGIN
  INSERT INTO items_fts(items_fts, rowid, title, body) VALUES ('delete', old.id, old.title, old.body);
END;
CREATE TRIGGER items_au AFTER UPDATE ON items BEGIN
  INSERT INTO items_fts(items_fts, rowid, title, body) VALUES ('delete', old.id, old.title, old.body);
  INSERT INTO items_fts(rowid, title, body) VALUES (new.id, new.title, new.body);
END;

CREATE TABLE topics (
  id             INTEGER PRIMARY KEY,
  name           TEXT NOT NULL,
  category       TEXT NOT NULL,            -- key into settings.categories
  obsidian_path  TEXT,                     -- relative to the work vault
  score          REAL,
  pills_json     TEXT NOT NULL DEFAULT '[]', -- [{label, weight}]
  archived_at    TEXT
);

CREATE TABLE topic_links (
  item_id     INTEGER NOT NULL REFERENCES items(id),
  topic_id    INTEGER NOT NULL REFERENCES topics(id),
  state       TEXT NOT NULL CHECK (state IN ('suggested', 'confirmed', 'wrong')),
  confidence  REAL,
  reason      TEXT,
  updated_at  TEXT NOT NULL DEFAULT (datetime('now')),
  PRIMARY KEY (item_id, topic_id)
);

CREATE TABLE topic_repos (
  topic_id  INTEGER NOT NULL REFERENCES topics(id),
  repo      TEXT NOT NULL,
  PRIMARY KEY (topic_id, repo)
);

CREATE TABLE topic_people (
  topic_id   INTEGER NOT NULL REFERENCES topics(id),
  person_id  INTEGER NOT NULL REFERENCES people(id),
  PRIMARY KEY (topic_id, person_id)
);

-- Calendar blocks. Life blocks (is_life_busy = 1) never have a title or item.
CREATE TABLE events (
  id            INTEGER PRIMARY KEY,
  item_id       INTEGER REFERENCES items(id),
  source_id     INTEGER NOT NULL REFERENCES sources(id),
  start         TEXT NOT NULL,
  end           TEXT NOT NULL,
  title         TEXT,                      -- NULL for life blocks
  category      TEXT NOT NULL,
  tentative     INTEGER NOT NULL DEFAULT 0,
  is_life_busy  INTEGER NOT NULL DEFAULT 0,
  buffer_min    INTEGER,                   -- per-block travel buffer override
  attendees     TEXT                       -- display string
);

CREATE TABLE priorities (
  item_id       INTEGER PRIMARY KEY REFERENCES items(id),
  quadrant      TEXT NOT NULL CHECK (quadrant IN ('now', 'make_room', 'quick', 'can_wait')),
  score         REAL NOT NULL,
  reasons_json  TEXT NOT NULL DEFAULT '[]', -- [{label, weight: red|yellow|green}]
  updated_at    TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE notes_lines (
  id           INTEGER PRIMARY KEY,
  page_date    TEXT NOT NULL,              -- YYYY-MM-DD
  position     INTEGER NOT NULL,
  kind         TEXT NOT NULL CHECK (kind IN ('note', 'do', 'research', 'delegated', 'life')),
  text         TEXT NOT NULL,
  target_json  TEXT,                       -- {type: topic|person|app, id?, label, offer?}
  state        TEXT NOT NULL DEFAULT 'draft' -- draft | incoming | held | pending | sent | confirmed | failed
);

CREATE TABLE reflections (
  id            INTEGER PRIMARY KEY,
  scope         TEXT NOT NULL CHECK (scope IN ('day', 'week', 'month', 'quarter')),
  period_start  TEXT NOT NULL,
  lens          TEXT NOT NULL CHECK (lens IN ('both', 'work', 'life')),
  obsidian_path TEXT NOT NULL,
  summary       TEXT,                      -- one line used by the level above
  UNIQUE (scope, period_start, lens)
);

CREATE TABLE carry (
  item_id     INTEGER NOT NULL REFERENCES items(id),
  period      TEXT NOT NULL,               -- day it was carried from
  action      TEXT CHECK (action IN ('tomorrow', 'schedule', 'let_go')),
  PRIMARY KEY (item_id, period)
);

CREATE TABLE outbox (
  id            INTEGER PRIMARY KEY,
  source_id     INTEGER NOT NULL REFERENCES sources(id),
  op            TEXT NOT NULL,
  payload_json  TEXT NOT NULL,
  state         TEXT NOT NULL DEFAULT 'pending' CHECK (state IN ('pending', 'sent', 'confirmed', 'failed')),
  error         TEXT,
  created_at    TEXT NOT NULL DEFAULT (datetime('now')),
  confirmed_at  TEXT
);
