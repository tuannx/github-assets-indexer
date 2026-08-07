CREATE TABLE IF NOT EXISTS resources (
  id                 TEXT PRIMARY KEY,
  source_id          TEXT NOT NULL REFERENCES sources(id),
  resource_type      TEXT NOT NULL,
  remote_id          TEXT NOT NULL,
  parent_resource_id TEXT,
  canonical_url      TEXT NOT NULL,
  title              TEXT,
  body               TEXT,
  remote_updated_at  TEXT,
  local_synced_at    TEXT NOT NULL,
  content_hash       TEXT NOT NULL,
  UNIQUE(source_id, resource_type, remote_id)
);

CREATE VIRTUAL TABLE IF NOT EXISTS fts_documents USING fts5(
  resource_id UNINDEXED,
  source_id UNINDEXED,
  resource_type UNINDEXED,
  title,
  body,
  tokenize='porter'
);
