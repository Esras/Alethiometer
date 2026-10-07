-- Priorities, three orders (app/docs/priorities.md). The 8b quadrants and their
-- stored scores go: scores are computed when they're read.
DROP TABLE priorities;

-- A soft date on local todos: the day you mean to do it (a local day).
ALTER TABLE items ADD COLUMN sched_day TEXT;
-- When the item was made in its own system (Jira's `created`), where the source says.
ALTER TABLE items ADD COLUMN source_created_at TEXT;

-- Set by you: H, M or L (Taskwarrior's priority values), or none.
ALTER TABLE topics ADD COLUMN priority TEXT CHECK (priority IN ('H', 'M', 'L'));

-- From the People note's `weight` (0 to 1): how much a todo this person asked for matters.
ALTER TABLE people ADD COLUMN weight REAL;

-- Tags from the vocabulary in settings.json (`priorities.tags`). A todo
-- inherits its topic's tags. `suggested` waits for you, like topic links.
CREATE TABLE item_tags (
  item_id  INTEGER NOT NULL REFERENCES items(id) ON DELETE CASCADE,
  tag      TEXT NOT NULL,
  state    TEXT NOT NULL CHECK (state IN ('suggested', 'confirmed', 'wrong')),
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  PRIMARY KEY (item_id, tag)
);
CREATE TABLE topic_tags (
  topic_id INTEGER NOT NULL REFERENCES topics(id) ON DELETE CASCADE,
  tag      TEXT NOT NULL,
  state    TEXT NOT NULL CHECK (state IN ('suggested', 'confirmed', 'wrong')),
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  PRIMARY KEY (topic_id, tag)
);

-- A todo held on a signal can carry its scheduled day too (local todos only).
ALTER TABLE held_todos ADD COLUMN sched TEXT;
