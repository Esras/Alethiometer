-- Local embeddings (embed/). Vectors live in sqlite-vec tables keyed by the
-- item's or topic's id; embed_meta remembers which model and text made each
-- one, so changed items and topics are re-embedded.
CREATE TABLE embed_meta (
  kind   TEXT NOT NULL,              -- item | topic
  id     INTEGER NOT NULL,
  model  TEXT NOT NULL,
  hash   TEXT NOT NULL,              -- of the text that was embedded
  at     TEXT NOT NULL DEFAULT (datetime('now', 'localtime')),
  PRIMARY KEY (kind, id)
);
CREATE VIRTUAL TABLE vec_items USING vec0(embedding float[384]);
CREATE VIRTUAL TABLE vec_topics USING vec0(embedding float[384]);
