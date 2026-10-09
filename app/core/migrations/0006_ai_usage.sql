CREATE TABLE ai_usage (
  id            INTEGER PRIMARY KEY,
  at            TEXT NOT NULL DEFAULT (datetime('now', 'localtime')),
  tier          TEXT NOT NULL,
  model         TEXT NOT NULL,
  input_tokens  INTEGER NOT NULL,
  output_tokens INTEGER NOT NULL
);
