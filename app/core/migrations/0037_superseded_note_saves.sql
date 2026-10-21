-- A whole-note save that went through makes earlier failed or queued saves of the same note moot.
DELETE FROM outbox
 WHERE op = 'obsidian.replace' AND state IN ('pending', 'failed')
   AND EXISTS (SELECT 1 FROM outbox later
                WHERE later.op = 'obsidian.replace' AND later.state = 'confirmed' AND later.id > outbox.id
                  AND later.source_id = outbox.source_id
                  AND json_extract(later.payload_json, '$.path') = json_extract(outbox.payload_json, '$.path'));
