CREATE TABLE ai_suggest_run (
  id          INTEGER PRIMARY KEY CHECK (id = 1),
  status      TEXT NOT NULL,
  message     TEXT,
  suggested   INTEGER NOT NULL,
  at          TEXT NOT NULL,
  last_ok_at  TEXT
);
