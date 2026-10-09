-- A todo drafted on an Incoming signal (22c), created only when the signal is
-- confirmed (with the topic it's confirmed to). Dismissing the signal discards
-- it (state 'discarded', so undo can bring it back).
CREATE TABLE held_todos (
  item_id    INTEGER PRIMARY KEY REFERENCES items(id),
  target     TEXT NOT NULL CHECK (target IN ('local', 'jira')),
  text       TEXT NOT NULL,
  due        TEXT,
  state      TEXT NOT NULL DEFAULT 'held' CHECK (state IN ('held', 'discarded')),
  created_at TEXT NOT NULL DEFAULT (datetime('now', 'localtime'))
);
