-- Topic categories, managed in the app. Seeded once from settings.json's `categories`.
CREATE TABLE categories (
  key       TEXT PRIMARY KEY,
  label     TEXT NOT NULL,
  color     TEXT NOT NULL,
  position  INTEGER NOT NULL DEFAULT 0
);
