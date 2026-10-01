-- People (design 17a/17b, 16c). A person is a note in the work vault
-- (People/<Name>.md); `queries::people::scan` mirrors the notes here.
-- Rows the sync creates per author handle (person_handles) stay as they are:
-- an author is matched to a note person through person_accounts at read time.
ALTER TABLE people ADD COLUMN note_path TEXT;
-- Palette index (0-255) from the note's `color`, if it sets one.
ALTER TABLE people ADD COLUMN color INTEGER;
ALTER TABLE people ADD COLUMN aliases_json TEXT NOT NULL DEFAULT '[]';
CREATE UNIQUE INDEX people_note ON people(note_path) WHERE note_path IS NOT NULL;

-- Each account a People note lists: slack (user id), email (lowercased),
-- jira (accountId), github (login).
CREATE TABLE person_accounts (
  kind       TEXT NOT NULL,
  account    TEXT NOT NULL,
  person_id  INTEGER NOT NULL REFERENCES people(id),
  PRIMARY KEY (kind, account)
);

-- "Not now" on a new-person suggestion (17a): hidden until `until`.
CREATE TABLE person_snoozes (
  kind     TEXT NOT NULL,
  account  TEXT NOT NULL,
  until    TEXT NOT NULL,
  PRIMARY KEY (kind, account)
);

CREATE INDEX item_messages_author ON item_messages(author_id) WHERE author_id IS NOT NULL;
