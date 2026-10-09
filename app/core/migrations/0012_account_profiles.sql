-- What a source says about one of its accounts (Slack: real name and profile
-- email, from users.info). Lets a Slack sender match a People note by email
-- and colour by email (16c), and names a new person's note (17a).
CREATE TABLE account_profiles (
  kind        TEXT NOT NULL,             -- slack
  account     TEXT NOT NULL,             -- the source's id (Slack user id)
  real_name   TEXT,
  email       TEXT,                      -- lowercased
  updated_at  TEXT NOT NULL DEFAULT (datetime('now')),
  PRIMARY KEY (kind, account)
);
