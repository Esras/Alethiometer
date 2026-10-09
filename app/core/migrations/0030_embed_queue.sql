-- Work items whose embedding may be missing or stale, queued by the
-- database when an item is added or its text changes, so the embedder
-- works through a queue instead of rescanning every item each pass
-- (app/docs/db-plan.md, Phase 4). `gen` moves on each change: the embedder
-- removes a row only if it's the generation it read, so an edit made while
-- it embedded stays queued.
CREATE TABLE embed_dirty (
  item_id INTEGER PRIMARY KEY,
  gen     INTEGER NOT NULL DEFAULT 0
);

-- Everything not embedded yet.
INSERT INTO embed_dirty (item_id)
  SELECT i.id FROM items i JOIN sources s ON s.id = i.source_id
   WHERE s.tag = 'work' AND NOT EXISTS (SELECT 1 FROM embed_meta m WHERE m.kind = 'item' AND m.id = i.id);

CREATE TRIGGER embed_dirty_new AFTER INSERT ON items
WHEN (SELECT tag FROM sources WHERE id = NEW.source_id) = 'work'
BEGIN
  INSERT INTO embed_dirty (item_id) VALUES (NEW.id) ON CONFLICT (item_id) DO UPDATE SET gen = gen + 1;
END;

CREATE TRIGGER embed_dirty_text AFTER UPDATE OF title, summary, body ON items
WHEN (OLD.title IS NOT NEW.title OR OLD.summary IS NOT NEW.summary OR OLD.body IS NOT NEW.body)
  AND (SELECT tag FROM sources WHERE id = NEW.source_id) = 'work'
BEGIN
  INSERT INTO embed_dirty (item_id) VALUES (NEW.id) ON CONFLICT (item_id) DO UPDATE SET gen = gen + 1;
END;
