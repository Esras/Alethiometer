-- When an item went from open to done, kept by the database so Reflect can list what a day finished.
-- Items first seen already done have no row: there's no honest time for them.
CREATE TABLE item_done (
  item_id INTEGER PRIMARY KEY REFERENCES items(id) ON DELETE CASCADE,
  done_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE TRIGGER item_done_set AFTER UPDATE OF status ON items
WHEN NEW.status = 'done' AND OLD.status IS NOT 'done'
BEGIN
  INSERT OR REPLACE INTO item_done (item_id) VALUES (NEW.id);
END;

CREATE TRIGGER item_done_cleared AFTER UPDATE OF status ON items
WHEN OLD.status = 'done' AND NEW.status IS NOT 'done'
BEGIN
  DELETE FROM item_done WHERE item_id = NEW.id;
END;
