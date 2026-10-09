-- How long a todo will take, in minutes: the small model's guess ('ai') or yours ('you', never overwritten). Local only, never sent to Jira.
CREATE TABLE todo_estimates (
  item_id    INTEGER PRIMARY KEY REFERENCES items(id),
  minutes    INTEGER NOT NULL CHECK (minutes > 0),
  by         TEXT NOT NULL CHECK (by IN ('ai', 'you')),
  updated_at TEXT NOT NULL DEFAULT (datetime('now', 'localtime'))
);
