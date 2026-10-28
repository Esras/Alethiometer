-- A close or reopen that the source already shows (done by hand there) is moot.
DELETE FROM outbox
 WHERE op IN ('jira.transition', 'obsidian.toggle_task') AND state IN ('pending', 'failed') AND item_id IS NOT NULL
   AND json_extract(payload_json, '$.done') IS NOT NULL
   AND json_extract(payload_json, '$.done') = (SELECT status = 'done' FROM items WHERE id = outbox.item_id);

CREATE TRIGGER items_status_moots_outbox AFTER UPDATE OF status ON items
WHEN NEW.status IS NOT OLD.status
BEGIN
  DELETE FROM outbox
   WHERE item_id = NEW.id AND op IN ('jira.transition', 'obsidian.toggle_task') AND state IN ('pending', 'failed')
     AND json_extract(payload_json, '$.done') = (NEW.status = 'done');
END;
