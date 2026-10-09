-- A topic's corpus revision: bumped by the database when its links change or
-- a linked item's text does, so the Notes suggestion index re-reads only the
-- topics that changed (app/docs/db-plan.md, Phase 4).
ALTER TABLE topics ADD COLUMN corpus_rev INTEGER NOT NULL DEFAULT 0;

CREATE TRIGGER topic_corpus_link_added AFTER INSERT ON topic_links
BEGIN
  UPDATE topics SET corpus_rev = corpus_rev + 1 WHERE id = NEW.topic_id;
END;

CREATE TRIGGER topic_corpus_link_removed AFTER DELETE ON topic_links
BEGIN
  UPDATE topics SET corpus_rev = corpus_rev + 1 WHERE id = OLD.topic_id;
END;

CREATE TRIGGER topic_corpus_link_changed AFTER UPDATE OF item_id, topic_id, state ON topic_links
WHEN OLD.item_id IS NOT NEW.item_id OR OLD.topic_id IS NOT NEW.topic_id OR OLD.state IS NOT NEW.state
BEGIN
  UPDATE topics SET corpus_rev = corpus_rev + 1 WHERE id IN (OLD.topic_id, NEW.topic_id);
END;

-- A sync rewrites every column of an item it updates; only a real change counts.
CREATE TRIGGER topic_corpus_item_text AFTER UPDATE OF title, summary, body ON items
WHEN OLD.title IS NOT NEW.title OR OLD.summary IS NOT NEW.summary OR OLD.body IS NOT NEW.body
BEGIN
  UPDATE topics SET corpus_rev = corpus_rev + 1 WHERE id IN (SELECT topic_id FROM topic_links WHERE item_id = NEW.id);
END;

-- Likewise for the item time copied onto its links (0028).
DROP TRIGGER items_at_to_links;
CREATE TRIGGER items_at_to_links AFTER UPDATE OF occurred_at, ended_at ON items
WHEN OLD.occurred_at IS NOT NEW.occurred_at OR OLD.ended_at IS NOT NEW.ended_at
BEGIN
  UPDATE topic_links SET item_at = coalesce(NEW.ended_at, NEW.occurred_at) WHERE item_id = NEW.id;
END;

-- Jira issues by source and time (the Notes index lists every one).
CREATE INDEX items_issues ON items(source_id, occurred_at) WHERE kind = 'issue';
