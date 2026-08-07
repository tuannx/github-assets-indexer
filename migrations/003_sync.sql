CREATE TABLE IF NOT EXISTS sync_jobs (
  id           TEXT PRIMARY KEY,
  source_id    TEXT NOT NULL REFERENCES sources(id),
  collection   TEXT NOT NULL,
  kind         TEXT NOT NULL,
  state        TEXT NOT NULL,
  attempts     INTEGER NOT NULL DEFAULT 0,
  available_at TEXT NOT NULL,
  created_at   TEXT NOT NULL,
  started_at   TEXT,
  finished_at  TEXT,
  last_error   TEXT
);

CREATE TABLE IF NOT EXISTS sync_collections (
  source_id       TEXT NOT NULL,
  collection      TEXT NOT NULL,
  cursor_json     TEXT,
  last_success_at TEXT,
  PRIMARY KEY (source_id, collection),
  FOREIGN KEY (source_id) REFERENCES sources(id)
);
